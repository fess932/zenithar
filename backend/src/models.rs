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
    /// Image carries an alpha channel (transparent PNG/WebP) → render frameless.
    #[serde(default)]
    pub has_alpha: bool,
    /// A sticker (pack item) → render bare: no frame, autoplay, no video controls.
    #[serde(default)]
    pub is_sticker: bool,
    /// If this sticker came from a pack, its pack's share slug — lets the recipient
    /// add the whole pack. None for ordinary (non-pack) attachments.
    #[serde(default)]
    pub pack_slug: Option<String>,
}

/// A "сохранёнка" — one image in a user's private saved collection. Holds its own
/// Storage blob (keyed by `id`), independent of any message. `public` exposes it
/// on the owner's profile. Shares the Attachment shape so the UI renders it the
/// same way.
#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct SavedItem {
    pub id: String,
    pub filename: String,
    pub content_type: String,
    pub size: i64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub has_thumb: bool,
    #[serde(default)]
    pub has_alpha: bool,
    #[serde(default)]
    pub is_sticker: bool,
    pub public: bool,
    pub created_at: i64,
}

/// A sticker/emoji pack — a named group of saved_items. Shared by `share_slug`:
/// anyone with the link copies the whole pack (its blobs) into their collection.
#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct SavedPack {
    pub id: String,
    pub owner_id: String,
    pub name: String,
    /// 'sticker' | 'gif' | 'saved' — which sub-list the pack shows under.
    pub kind: String,
    /// Exposed on the owner's profile for anyone to view + add.
    #[serde(default)]
    pub public: bool,
    pub cover_item_id: Option<String>,
    pub share_slug: String,
    pub created_at: i64,
}

/// A pack with its member items — the shape the pack list/share view returns.
#[derive(Clone, Debug, Serialize)]
pub struct PackWithItems {
    #[serde(flatten)]
    pub pack: SavedPack,
    pub items: Vec<SavedItem>,
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

/// Someone who reacted — id + their avatar, so the UI can show reactor faces
/// (not just a count).
#[derive(Clone, Debug, Serialize)]
pub struct Reactor {
    pub id: String,
    pub avatar: Option<String>,
}

/// Reactions for one emoji on a message: who reacted. The client derives the
/// count (`by.len()`), renders reactor avatars, and highlights it when its own id
/// is in `by`. Grouped per emoji and embedded in [`ChatMessage`].
#[derive(Clone, Debug, Serialize)]
pub struct Reaction {
    pub emoji: String,
    pub by: Vec<Reactor>,
}

/// A persisted / broadcast chat message. Attachments (0–5) and reactions are
/// loaded separately and embedded, so this is not a direct `FromRow`.
#[derive(Clone, Debug, Serialize)]
pub struct ChatMessage {
    pub id: String, // ULID, app-generated so we can broadcast before the DB commit
    pub room_id: String,
    pub author_id: String,
    pub author_name: String,
    /// The author's current avatar (emoji or `"photo:<millis>"`), resolved at read
    /// time so it stays current; None → the client renders a default emoji.
    pub author_avatar: Option<String>,
    pub body: String,
    /// The quoted message, if this is a reply. The parent id is persisted in the
    /// `messages.reply_to` column (see [`crate::writer`]).
    pub reply_to: Option<ReplyPreview>,
    pub client_msg_id: Option<String>,
    pub created_at: i64,        // unix millis
    pub edited_at: Option<i64>, // set when the author edits the body
    pub attachments: Vec<Attachment>,
    /// Emoji reactions, grouped per emoji. Empty for a brand-new message.
    pub reactions: Vec<Reaction>,
    /// Sticker id when this is a sticker message (body is then empty); the client
    /// renders the matching bundled animation. None for ordinary messages.
    pub sticker: Option<String>,
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
    /// Last message in the room, for the chat-list preview (Telegram-style row).
    /// All `None` when the room has no messages yet. `last_body` may be empty for
    /// an attachment-only message (the UI shows a generic marker then).
    #[sqlx(default)]
    pub last_at: Option<i64>,
    #[sqlx(default)]
    pub last_body: Option<String>,
    #[sqlx(default)]
    pub last_author: Option<String>,
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
    /// Leave the active room (e.g. back to the mobile chat list): marks it read up
    /// to now, then clears the active room so further messages there arrive as an
    /// unread badge instead of being auto-read.
    Leave,
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
        /// A sticker id (e.g. "heart"); the message is a sticker, body is empty.
        #[serde(default)]
        sticker: Option<String>,
    },
    /// Edit a message's body (author only).
    Edit { id: String, body: String },
    /// Delete a message (author, or any admin).
    Delete { id: String },
    /// Toggle one emoji reaction on a message (anyone in the room).
    React { id: String, emoji: String },
    /// Read receipt: the sender has read `room_id` up to timestamp `at`.
    Read { room_id: String, at: i64 },
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
    /// Per-room unread counts, sent on connect so the chat list survives a reload.
    UnreadCounts {
        counts: std::collections::HashMap<String, i64>,
    },
    /// A message landed in a room you're not viewing — bump its unread badge.
    Unread { room_id: String },
    /// A message's body was edited (live update for viewers of that room).
    MessageEdited {
        id: String,
        room_id: String,
        body: String,
        edited_at: i64,
    },
    /// A message was deleted (viewers of that room remove it).
    MessageDeleted { id: String, room_id: String },
    /// A message's reactions changed (live update for viewers of that room).
    MessageReaction {
        id: String,
        room_id: String,
        reactions: Vec<Reaction>,
    },
    /// Someone reacted to YOUR message — a light, quiet nudge (not a message).
    /// Targeted at the message author only.
    ReactionNotice {
        room_id: String,
        message_id: String,
        emoji: String,
        from_name: String,
    },
    /// A principal advanced their read pointer in a room → live ✓✓ for authors.
    Read {
        room_id: String,
        principal_id: String,
        at: i64,
    },
    /// Snapshot on join: the newest timestamp OTHERS have read to in this room, so
    /// existing sent messages render ✓/✓✓ correctly on load.
    ReadState {
        room_id: String,
        others_read_at: i64,
    },
    /// The caller's room list changed (e.g. someone opened a DM with them) —
    /// refetch `/api/rooms`. Sent targeted at the affected principal.
    RoomsChanged,
}

/// An addressed signaling frame fanned out over the `signal` broadcast channel.
/// Sockets deliver it when it targets their principal, or — for room-scoped
/// frames (`target: None`) — when it matches their active room (minus `exclude`).
#[derive(Clone, Debug)]
pub struct Signal {
    pub room_id: String,
    pub target: Option<String>,
    pub exclude: Option<String>,
    /// Deliver to every employee NOT currently viewing `room_id` (cross-room),
    /// on top of the normal room-scoped delivery. Used to ring all employees when
    /// an anonymous client starts a call, so anyone on the team can pick it up.
    pub all_employees: bool,
    pub frame: Outbound,
}
