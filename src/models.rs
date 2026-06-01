use serde::{Deserialize, Serialize};

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct ClientMessage {
    pub action: String,
    pub player_id: String,
    pub mmr: u16,
}

#[allow(dead_code)]
#[derive(Debug, Serialize)]
pub struct ServerMessage {
    pub status: String,
    pub message: String,
}