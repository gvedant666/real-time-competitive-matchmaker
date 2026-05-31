use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
};
use tracing::{info, warn};
use crate::models::{ClientMessage, ServerMessage};


pub async fn ws_handler(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(handle_socket)
}

async fn handle_socket(mut socket: WebSocket) {
    info!("New WebSocket connection established.");

    while let Some(Ok(msg)) = socket.recv().await {
        if let Message::Text(text) = msg {
            // Attempt to parse the incoming JSON
            match serde_json::from_str::<ClientMessage>(&text) {
                Ok(client_msg) => {
                    info!("Received valid request: {:?}", client_msg);

                    // Matchmaking logic would go here. For now, we just log the request and send a dummy response.

                    // Send a dummy response back for now
                    let response = ServerMessage {
                        status: "queued".to_string(),
                        message: format!("Player {} added to pool.", client_msg.player_id),
                    };
                    let response_text = serde_json::to_string(&response).unwrap();
                    let _ = socket.send(Message::Text(response_text)).await;
                }
                Err(e) => {
                    warn!("Failed to parse message: {}", e);
                }
            }
        }
    }

    info!("Client disconnected.");
}