use std::time::Duration;

use anyhow::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;

use crate::models::ChatMessage;

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
        "SELECT id, room_id, author, body, client_msg_id, created_at
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
