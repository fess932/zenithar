use std::time::Duration;

use sqlx::SqlitePool;
use tokio::sync::{mpsc, oneshot};
use tokio::time::sleep;
use tracing::{error, warn};

use crate::models::ChatMessage;

/// Flush the batch when whichever comes first:
const MAX_BATCH: usize = 16; // 10–20 messages buffered, or
const MAX_DELAY: Duration = Duration::from_millis(50); // ~50ms since the first one.

/// A unit of work for the writer. `ack` (optional) fires once the batch
/// containing this message has been committed — that's the "saved" signal.
pub struct WriteCmd {
    pub msg: ChatMessage,
    pub ack: Option<oneshot::Sender<()>>,
}

/// Handle used by WS handlers to enqueue writes. Bounded → backpressure: if the
/// writer falls behind, senders await instead of growing memory unbounded.
pub type WriteTx = mpsc::Sender<WriteCmd>;

pub fn channel() -> (WriteTx, mpsc::Receiver<WriteCmd>) {
    mpsc::channel(1024)
}

/// The batching writer loop. Uses the single-connection write pool, so every
/// transaction here is serialised — exactly what SQLite wants.
pub async fn run(pool: SqlitePool, mut rx: mpsc::Receiver<WriteCmd>) {
    loop {
        // Block until at least one message is available (or the channel closes).
        let first = match rx.recv().await {
            Some(cmd) => cmd,
            None => break, // all senders dropped → shutdown
        };

        let mut batch = vec![first];
        let deadline = sleep(MAX_DELAY);
        tokio::pin!(deadline);

        // Fill the batch until we hit the count cap or the time window expires.
        while batch.len() < MAX_BATCH {
            tokio::select! {
                _ = &mut deadline => break,
                maybe = rx.recv() => match maybe {
                    Some(cmd) => batch.push(cmd),
                    None => break,
                },
            }
        }

        // Commit the whole batch in one transaction (one fsync).
        if let Err(e) = write_batch(&pool, &batch).await {
            // Phase 0: log and keep going. Persistence is best-effort vs. realtime.
            error!(error = %e, count = batch.len(), "message batch write failed");
        }

        // Notify waiters (ack) after the commit settled.
        for cmd in batch {
            if let Some(ack) = cmd.ack {
                let _ = ack.send(());
            }
        }
    }
    warn!("writer task stopped (channel closed)");
}

async fn write_batch(pool: &SqlitePool, batch: &[WriteCmd]) -> sqlx::Result<()> {
    let mut tx = pool.begin().await?;
    for cmd in batch {
        let m = &cmd.msg;
        sqlx::query(
            "INSERT INTO messages
               (id, room_id, author_id, author_name, body, client_msg_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(client_msg_id) DO NOTHING",
        )
        .bind(&m.id)
        .bind(&m.room_id)
        .bind(&m.author_id)
        .bind(&m.author_name)
        .bind(&m.body)
        .bind(m.client_msg_id.as_deref())
        .bind(m.created_at)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await
}
