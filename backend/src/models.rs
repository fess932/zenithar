use serde::{Deserialize, Serialize};

/// Metadata for an uploaded file. Bytes live in `Storage` keyed by `id`; images
/// also have a `<id>.thumb` thumbnail (`has_thumb`) and pixel dimensions.
#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Attachment {
    pub id: String,
    pub filename: String,
    pub content_type: String,
    pub size: i64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub has_thumb: bool,
}

/// A persisted / broadcast chat message. Attachments (0–5) are loaded separately
/// and embedded, so this is not a direct `FromRow`.
#[derive(Clone, Debug, Serialize)]
pub struct ChatMessage {
    pub id: String, // ULID, app-generated so we can broadcast before the DB commit
    pub room_id: String,
    pub author_id: String,
    pub author_name: String,
    pub body: String,
    pub client_msg_id: Option<String>,
    pub created_at: i64, // unix millis
    pub attachments: Vec<Attachment>,
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

/// A live participant in a call (for the call-state UI).
#[derive(Clone, Debug, Serialize)]
pub struct CallParticipant {
    pub id: String,
    pub name: String,
}

/// Client → server WebSocket frames. The author is taken from the authenticated
/// identity, never from the client. Chat frames are room-broadcast; `call-*`
/// frames drive the per-call WebRTC signaling (see [`Outbound`]).
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Inbound {
    /// Switch the socket's active room (employees only switch; clients are pinned).
    Join { room_id: String },
    /// Send a message to the active room (optionally carrying up to 5 attachments).
    Msg {
        body: String,
        #[serde(default)]
        client_msg_id: Option<String>,
        #[serde(default)]
        attachment_ids: Vec<String>,
    },
    /// Start (or join) the call in a room. The server replies with `call-offer`.
    CallStart { room_id: String },
    /// SDP answer to the server's offer (the server is always the offerer).
    CallAnswer { call_id: String, sdp: String },
    /// A trickled ICE candidate (JSON of `RTCIceCandidateInit`).
    CallIce { call_id: String, candidate: String },
    /// Leave / hang up the call.
    CallLeave { call_id: String },
}

/// Server → client WebSocket frames.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Outbound {
    /// Full transcript for a room, sent on connect and after a join.
    History {
        room_id: String,
        messages: Vec<ChatMessage>,
    },
    /// A single new message for the active room.
    Message { message: Box<ChatMessage> },
    /// A call just started in your room — show a ring/join prompt.
    CallRinging {
        call_id: String,
        room_id: String,
        from: String,
        from_name: String,
    },
    /// SDP offer from the server's PeerConnection (answer with `call-answer`).
    CallOffer { call_id: String, sdp: String },
    /// A trickled ICE candidate from the server (JSON of `RTCIceCandidateInit`).
    CallIce { call_id: String, candidate: String },
    /// Participants currently in the call (join/leave updates).
    CallState {
        call_id: String,
        participants: Vec<CallParticipant>,
    },
    /// The call is over (last participant left).
    CallEnded { call_id: String },
}

/// An addressed signaling frame fanned out over the `signal` broadcast channel.
/// Sockets deliver it when it targets their principal, or — for room-scoped
/// frames (`target: None`) — when it matches their active room (minus `exclude`).
#[derive(Clone, Debug)]
pub struct Signal {
    pub room_id: String,
    pub target: Option<String>,
    pub exclude: Option<String>,
    pub frame: Outbound,
}
