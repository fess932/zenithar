use sqlx::SqlitePool;
use tokio::sync::broadcast;

use crate::models::ChatMessage;
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
    /// Set Secure on auth cookies (enable behind TLS).
    pub secure_cookies: bool,
}
