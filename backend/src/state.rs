use std::sync::Arc;

use sqlx::SqlitePool;
use tokio::sync::broadcast;

use crate::calls::CallRegistry;
use crate::models::{ChatMessage, ClientNotice, Signal};
use crate::presence::PresenceRegistry;
use crate::ratelimit::Limits;
use crate::storage::Storage;
use crate::writer::WriteTx;

/// Shared application state. `reads` is the read-only pool; `db` is the
/// single-writer pool (also used by the batching message writer).
#[derive(Clone)]
pub struct AppState {
    pub writes: WriteTx,
    /// Live messages fan out here; each socket forwards only its active room.
    pub broadcast: broadcast::Sender<ChatMessage>,
    pub reads: SqlitePool,
    pub db: SqlitePool,
    /// Blob backend for attachments (disk now; S3-swappable later).
    pub storage: Arc<dyn Storage>,
    /// Addressed WebRTC signaling fan-out (call offers/answers/ICE/state).
    pub signal: broadcast::Sender<Signal>,
    /// Live voice calls; the server is the WebRTC peer in the media path.
    pub calls: Arc<CallRegistry>,
    /// Heads-up fan-out for new anonymous-client messages (delivered to all
    /// employee sockets, cross-room).
    pub notify: broadcast::Sender<ClientNotice>,
    /// Who's online right now (Phase 7 presence).
    pub presence: Arc<PresenceRegistry>,
    /// Abuse control: login + upload rate limiters (Phase 7).
    pub limits: Arc<Limits>,
    /// Set Secure on auth cookies (enable behind TLS).
    pub secure_cookies: bool,
}
