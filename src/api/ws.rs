use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, State},
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tracing::{error, info};

use crate::engine::state::EngineState;
use crate::models::{ClientMessage, ServerMessage};

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(engine): State<Arc<EngineState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, engine))
}

async fn handle_socket(socket: WebSocket, engine: Arc<EngineState>) {
    info!("New client connected to WebSocket.");

    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (tx_out, mut rx_out) = tokio::sync::mpsc::channel::<Message>(100);

    tokio::spawn(async move {
        while let Some(msg) = rx_out.recv().await {
            if ws_sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = ws_receiver.next().await {
        if let Message::Text(text) = msg {
            match serde_json::from_str::<ClientMessage>(&text) {
                Ok(client_msg) => {
                    if client_msg.action == "join_queue" {
                        info!("Player {} (MMR: {}) joining queue...", client_msg.player_id, client_msg.mmr);
                        
                        let (tx, rx) = tokio::sync::oneshot::channel();
                        
                        // push the player
                        match engine.add_player(client_msg.player_id.clone(), client_msg.mmr, tx) {
                            Ok(arena_index) => {
                                let response = ServerMessage {
                                    status: "queued".to_string(),
                                    message: format!("Assigned Arena Index: {}", arena_index),
                                };
                                let _ = tx_out.send(Message::Text(serde_json::to_string(&response).unwrap())).await;

                                let tx_out_clone = tx_out.clone();
                                tokio::spawn(async move {
                                    if let Ok(queue_event) = rx.await {
                                        match queue_event {
                                            crate::engine::balancer::QueueEvent::MatchFound(match_details) => {
                                                if let Ok(response_text) = serde_json::to_string(&match_details) {
                                                    let _ = tx_out_clone.send(Message::Text(response_text)).await;
                                                }
                                            }
                                            crate::engine::balancer::QueueEvent::Timeout => {
                                                let timeout_response = ServerMessage {
                                                    status: "timeout".to_string(),
                                                    message: "No match found within the maximum queue time limit.".to_string(),
                                                };
                                                let _ = tx_out_clone.send(Message::Text(serde_json::to_string(&timeout_response).unwrap())).await;
                                            }
                                        }
                                    }
                                });
                            }
                            Err(e) => {
                                error!("Failed to add player: {}", e);
                                let response = ServerMessage {
                                    status: "error".to_string(),
                                    message: "Arena is completely full".to_string(),
                                };
                                let _ = tx_out.send(Message::Text(serde_json::to_string(&response).unwrap())).await;
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to parse message: {}", e);
                }
            }
        }
    }
    info!("Client disconnected.");
}