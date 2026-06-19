use serde::{Deserialize, Serialize};

/// A persisted / broadcast chat message.
#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct ChatMessage {
    pub id: String, // ULID, app-generated so we can broadcast before the DB commit
    pub room_id: String,
    pub author_id: String,
    pub author_name: String,
    pub body: String,
    pub client_msg_id: Option<String>,
    pub created_at: i64, // unix millis
}

/// A room the current principal may access. `title` is the client's display name
/// for client rooms; `None` for the common room (the frontend localizes it).
#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct RoomSummary {
    pub id: String,
    pub kind: String,
    pub title: Option<String>,
    pub created_at: i64,
}

/// Client → server WebSocket frames. The author is taken from the authenticated
/// identity, never from the client.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Inbound {
    /// Switch the socket's active room (employees only switch; clients are pinned).
    Join { room_id: String },
    /// Send a message to the active room.
    Msg {
        body: String,
        #[serde(default)]
        client_msg_id: Option<String>,
    },
}

/// Server → client WebSocket frames.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Outbound {
    /// Full transcript for a room, sent on connect and after a join.
    History {
        room_id: String,
        messages: Vec<ChatMessage>,
    },
    /// A single new message for the active room.
    Message { message: ChatMessage },
}
