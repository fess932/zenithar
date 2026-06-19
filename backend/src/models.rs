use serde::{Deserialize, Serialize};

/// A persisted / broadcast chat message.
#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct ChatMessage {
    pub id: String, // ULID, app-generated so we can broadcast before the DB commit
    pub room_id: String,
    pub author: String,
    pub body: String,
    pub client_msg_id: Option<String>,
    pub created_at: i64, // unix millis
}

/// What a client sends over the WebSocket.
#[derive(Debug, Deserialize)]
pub struct IncomingMessage {
    pub body: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub client_msg_id: Option<String>,
}
