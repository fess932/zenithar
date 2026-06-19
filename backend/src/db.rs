use std::time::Duration;

use anyhow::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;

use ulid::Ulid;

use crate::models::{ChatMessage, RoomSummary};

const MIGRATION: &str = include_str!("../migrations/0001_init.sql");

/// The single-writer pool (max 1 connection). SQLite serialises writes, so we
/// funnel all of them through one connection — that's what lets the writer task
/// batch many messages into one transaction. Also applies PRAGMAs + migrations.
pub async fn open_writer(path: &str) -> Result<SqlitePool> {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal) // readers don't block the writer
        .synchronous(SqliteSynchronous::Normal) // with WAL: no corruption, lose at most last txn
        .busy_timeout(Duration::from_secs(5))
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await?;

    // Idempotent schema (CREATE TABLE IF NOT EXISTS ...). raw_sql runs all
    // statements in the file; good enough until we add a migration tracker.
    sqlx::raw_sql(MIGRATION).execute(&pool).await?;
    Ok(pool)
}

/// A read-only pool. WAL lets these run concurrently with the write batch, so
/// reads never wait on persistence.
pub async fn open_readers(path: &str, size: u32) -> Result<SqlitePool> {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .busy_timeout(Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(size.max(1))
        .connect_with(opts)
        .await?;
    Ok(pool)
}

/// Most recent messages in a room, returned oldest-first.
pub async fn recent_messages(
    pool: &SqlitePool,
    room_id: &str,
    limit: i64,
) -> Result<Vec<ChatMessage>> {
    let mut rows = sqlx::query_as::<_, ChatMessage>(
        "SELECT id, room_id, author_id, author_name, body, client_msg_id, created_at
         FROM messages WHERE room_id = ?1
         ORDER BY id DESC LIMIT ?2",
    )
    .bind(room_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    rows.reverse(); // query was DESC for the LIMIT; hand back oldest-first
    Ok(rows)
}

/// The dedicated room for a client, creating it on first need. Idempotent —
/// callers (principal creation, client connect) can call it freely. Uses the
/// write pool.
pub async fn ensure_client_room(write: &SqlitePool, client_id: &str) -> sqlx::Result<String> {
    if let Some((id,)) = sqlx::query_as::<_, (String,)>("SELECT id FROM rooms WHERE client_id = ?1")
        .bind(client_id)
        .fetch_optional(write)
        .await?
    {
        return Ok(id);
    }
    let id = Ulid::new().to_string();
    sqlx::query(
        "INSERT INTO rooms (id, kind, client_id, created_at) VALUES (?1, 'client', ?2, ?3)",
    )
    .bind(&id)
    .bind(client_id)
    .bind(crate::now_millis())
    .execute(write)
    .await?;
    Ok(id)
}

/// Whether a room exists (employee join validation; employees may open any room).
pub async fn room_exists(reads: &SqlitePool, room_id: &str) -> sqlx::Result<bool> {
    let row = sqlx::query_as::<_, (i64,)>("SELECT 1 FROM rooms WHERE id = ?1 LIMIT 1")
        .bind(room_id)
        .fetch_optional(reads)
        .await?;
    Ok(row.is_some())
}

/// Rooms an employee can see: common first, then each client room (titled with
/// the client's display name), oldest client first.
pub async fn list_rooms_for_user(reads: &SqlitePool) -> sqlx::Result<Vec<RoomSummary>> {
    let rooms = sqlx::query_as::<_, RoomSummary>(
        "SELECT r.id, r.kind, p.display_name AS title, r.created_at
         FROM rooms r
         LEFT JOIN principals p ON p.id = r.client_id
         ORDER BY (r.kind = 'common') DESC, r.created_at ASC, r.id ASC",
    )
    .fetch_all(reads)
    .await?;
    Ok(rooms)
}
