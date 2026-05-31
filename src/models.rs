use serde::{Deserialize, Serialize};


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