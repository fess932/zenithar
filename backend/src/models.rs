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

/// A compact preview of the message a reply quotes (Telegram-style). Derived at
/// read time from the parent row, so it always renders even if the original has
/// scrolled out of the loaded window. `id` lets the client jump to the original.
#[derive(Clone, Debug, Serialize)]
pub struct ReplyPreview {
    pub id: String,
    pub author_name: String,
    pub body: String,
    pub has_attachment: bool,
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
    /// The quoted message, if this is a reply. The parent id is persisted in the
    /// `messages.reply_to` column (see [`crate::writer`]).
    pub reply_to: Option<ReplyPreview>,
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
    /// The client principal that owns a client room (None for the common room);
    /// lets the UI show that client's online dot.
    pub client_id: Option<String>,
    pub created_at: i64,
}

/// One online principal, for the presence snapshot sent on connect.
#[derive(Clone, Debug, Serialize)]
pub struct PresenceEntry {
    pub id: String,
    pub kind: String,
}

/// A cross-room heads-up for employees: an anonymous client just wrote in their
/// room. Fanned out to every employee socket regardless of the room they're
/// viewing — the full message itself still only enters its own room's transcript.
#[derive(Clone, Debug, Serialize)]
pub struct ClientNotice {
    pub room_id: String,
    pub from_name: String,
    pub preview: String,
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
        /// Id of the message being replied to, if any.
        #[serde(default)]
        reply_to: Option<String>,
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
    /// The server's single host ICE candidate rides inside this SDP, so there is
    /// no separate server→client `call-ice` trickle (the client still trickles
    /// its own candidates to us via the inbound `call-ice`).
    CallOffer { call_id: String, sdp: String },
    /// Participants currently in the call (join/leave updates).
    CallState {
        call_id: String,
        participants: Vec<CallParticipant>,
    },
    /// The call is over (last participant left).
    CallEnded { call_id: String },
    /// A new message landed in an anonymous client room (employees only).
    ClientNotice { notice: ClientNotice },
    /// Full set of currently-online principals (sent once on connect).
    PresenceSnapshot { online: Vec<PresenceEntry> },
    /// A principal came online / went offline.
    Presence {
        id: String,
        kind: String,
        online: bool,
    },
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
