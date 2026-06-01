use std::sync::Arc;
use futures_util::{StreamExt, SinkExt};
use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, error};

use real_time_competitive_matchmaker::engine::config::EngineConfig;
use real_time_competitive_matchmaker::engine::state::EngineState;
use real_time_competitive_matchmaker::engine::worker::{
    initialize_decay_lut, spawn_tick_thread, spawn_worker_thread
};

use real_time_competitive_matchmaker::models::ClientMessage;
use real_time_competitive_matchmaker::engine::balancer::QueueEvent;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    
    let config = EngineConfig::load();
    initialize_decay_lut(&config);
    
    let state = Arc::new(EngineState::new(config));
    
    tokio::spawn(spawn_tick_thread(Arc::clone(&state)));
    
    for _ in 0..4 {
        spawn_worker_thread(Arc::clone(&state));
    }
    
    let server_address = "127.0.0.1:8080";
    let listener = TcpListener::bind(server_address).await.expect("Failed to bind TCP listener");
    info!("Matchmaking WebSocket server listening running on {}", server_address);
    
    while let Ok((stream, _)) = listener.accept().await {
        let state_clone = Arc::clone(&state);
        
        tokio::spawn(async move {
            let ws_stream = match accept_async(stream).await {
                Ok(ws) => ws,
                Err(e) => {
                    error!("WebSocket connection failed: {}", e);
                    return;
                }
            };
            
            let (mut ws_sender, mut ws_receiver) = ws_stream.split();
            
            // 1. Create a channel to handle all outgoing messages
            let (tx_out, mut rx_out) = tokio::sync::mpsc::channel::<Message>(100);

            // 2. Spawn a dedicated background task to write to the WebSocket
            tokio::spawn(async move {
                while let Some(msg) = rx_out.recv().await {
                    if ws_sender.send(msg).await.is_err() {
                        break;
                    }
                }
            });
            
            // 3. The main read loop is now completely unblocked
            while let Some(Ok(Message::Text(text))) = ws_receiver.next().await {
                if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) {
                    
                    if client_msg.action == "join_queue" {
                        info!("-> JOIN: Player {} (MMR: {})", client_msg.player_id, client_msg.mmr);
                        
                        let (tx, rx) = tokio::sync::oneshot::channel();
                        
                        if state_clone.add_player(client_msg.player_id, client_msg.mmr, tx).is_ok() {
                            
                            let tx_out_clone = tx_out.clone();
                            
                            // 4. SPAWN THE WAITER IN THE BACKGROUND
                            // This allows the read loop to instantly process the next bot!
                            tokio::spawn(async move {
                                if let Ok(queue_event) = rx.await {
                                    match queue_event {
                                        QueueEvent::MatchFound(match_details) => {
                                            if let Ok(response_text) = serde_json::to_string(&match_details) {
                                                let _ = tx_out_clone.send(Message::Text(response_text.into())).await;
                                            }
                                        }
                                        QueueEvent::Timeout => {
                                            let timeout_msg = "{\"status\":\"timeout\",\"message\":\"No match found within the maximum queue time limit.\"}";
                                            let _ = tx_out_clone.send(Message::Text(timeout_msg.into())).await;
                                        }
                                    }
                                }
                            });
                        }
                    }
                }
            }
        });
    }
}