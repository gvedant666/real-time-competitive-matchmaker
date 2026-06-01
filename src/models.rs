use serde::{Deserialize, Serialize};


#[derive(Deserialize, Debug)]
pub struct ClientMessage {
    pub action: String,
    pub player_id: String,
    pub mmr: u16,
}

#[derive(Debug, Serialize)]
pub struct ServerMessage {
    pub status: String,
    pub message: String,
}