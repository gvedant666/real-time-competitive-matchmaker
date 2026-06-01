use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, State},
    response::IntoResponse,
};
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

async fn handle_socket(mut socket: WebSocket, engine: Arc<EngineState>) {
    info!("New client connected to WebSocket.");

    while let Some(Ok(msg)) = socket.recv().await {
        if let Message::Text(text) = msg {
            match serde_json::from_str::<ClientMessage>(&text) {
                Ok(client_msg) => {
                    if client_msg.action == "join_queue" {
                        info!("Player {} (MMR: {}) joining queue...", client_msg.player_id, client_msg.mmr);
                        
                        // push the player
                        match engine.add_player(client_msg.mmr) {
                            Ok(arena_index) => {
                                let response = ServerMessage {
                                    status: "queued".to_string(),
                                    message: format!("Assigned Arena Index: {}", arena_index),
                                };
                                let _ = socket.send(Message::Text(serde_json::to_string(&response).unwrap())).await;
                            }
                            Err(e) => {
                                error!("Failed to add player: {}", e);
                                let response = ServerMessage {
                                    status: "error".to_string(),
                                    message: "Arena is completely full".to_string(),
                                };
                                let _ = socket.send(Message::Text(serde_json::to_string(&response).unwrap())).await;
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