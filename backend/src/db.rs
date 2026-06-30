use std::time::Duration;

use anyhow::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;

use ulid::Ulid;

use crate::models::{Attachment, ChatMessage, ReplyPreview, RoomSummary};

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

    // Versioned migrations (tracked in `_sqlx_migrations`): each file under
    // ./migrations runs exactly once, in order — so additive ALTERs live in their
    // own .sql file instead of a re-run-every-boot script.
    sqlx::migrate!().run(&pool).await?;
    Ok(pool)
}

/// The read pool. WAL lets these run concurrently with the write batch, so reads
/// never wait on persistence.
///
/// NOTE: do **not** open these `read_only`. A SQLite read-only connection cannot
/// read uncheckpointed `-wal` content (it needs write access to the `-shm`
/// wal-index), so it would only ever see the last-checkpointed snapshot — making
/// freshly written messages invisible until a checkpoint. We open normal
/// (read-capable) WAL connections and simply never issue writes through them.
pub async fn open_readers(path: &str, size: u32) -> Result<SqlitePool> {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5))
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(size.max(1))
        .connect_with(opts)
        .await?;
    Ok(pool)
}

#[derive(sqlx::FromRow)]
struct MsgRow {
    id: String,
    room_id: String,
    author_id: String,
    author_name: String,
    // The author's current avatar, joined live from principals (None if unset).
    author_avatar: Option<String>,
    body: String,
    client_msg_id: Option<String>,
    created_at: i64,
    edited_at: Option<i64>,
    // Quoted parent (NULL unless this is a reply), filled by the self-join.
    reply_id: Option<String>,
    reply_author: Option<String>,
    reply_body: Option<String>,
    reply_has_att: i64,
}

impl MsgRow {
    /// Fold the joined parent columns into a `ReplyPreview` (None unless a reply).
    fn reply_preview(&self) -> Option<ReplyPreview> {
        self.reply_id.as_ref().map(|id| ReplyPreview {
            id: id.clone(),
            author_name: self.reply_author.clone().unwrap_or_default(),
            body: self.reply_body.clone().unwrap_or_default(),
            has_attachment: self.reply_has_att != 0,
        })
    }
}

/// Attachment row carrying its owning message id (for grouping into messages).
#[derive(sqlx::FromRow)]
struct MsgAttRow {
    message_id: String,
    id: String,
    filename: String,
    content_type: String,
    size: i64,
    width: Option<i64>,
    height: Option<i64>,
    has_thumb: bool,
}

/// Most recent messages in a room (oldest-first), each with its attachments (0–5).
pub async fn recent_messages(
    pool: &SqlitePool,
    room_id: &str,
    limit: i64,
) -> Result<Vec<ChatMessage>> {
    messages_before(pool, room_id, limit, None).await
}

/// A page of a room's messages, oldest-first, each with its attachments. When
/// `before` is given, only messages strictly older than that id are returned
/// (ULIDs are time-sortable), so a client paginates backwards by passing the id
/// of the oldest message it already has. With `before = None` this is the most
/// recent page (used for the connect-time transcript).
pub async fn messages_before(
    pool: &SqlitePool,
    room_id: &str,
    limit: i64,
    before: Option<&str>,
) -> Result<Vec<ChatMessage>> {
    const COLS: &str =
        "SELECT m.id, m.room_id, m.author_id, m.author_name, ap.avatar AS author_avatar,
                m.body, m.client_msg_id, m.created_at,
                m.edited_at,
                p.id AS reply_id, p.author_name AS reply_author, p.body AS reply_body,
                EXISTS(SELECT 1 FROM attachments a WHERE a.message_id = p.id) AS reply_has_att
         FROM messages m
         LEFT JOIN messages p ON p.id = m.reply_to
         LEFT JOIN principals ap ON ap.id = m.author_id ";

    let rows = match before {
        Some(b) => {
            // SQL is built only from our own constants (no user input) — safe.
            sqlx::query_as::<_, MsgRow>(sqlx::AssertSqlSafe(format!(
                "{COLS} WHERE m.room_id = ?1 AND m.id < ?3 ORDER BY m.id DESC LIMIT ?2"
            )))
            .bind(room_id)
            .bind(limit)
            .bind(b)
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query_as::<_, MsgRow>(sqlx::AssertSqlSafe(format!(
                "{COLS} WHERE m.room_id = ?1 ORDER BY m.id DESC LIMIT ?2"
            )))
            .bind(room_id)
            .bind(limit)
            .fetch_all(pool)
            .await?
        }
    };

    if rows.is_empty() {
        return Ok(Vec::new());
    }

    // Attachments for exactly the messages we just loaded (IN the loaded ids).
    let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    let placeholders = (1..=ids.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let att_sql = format!(
        "SELECT message_id, id, filename, content_type, size, width, height, has_thumb
         FROM attachments WHERE message_id IN ({placeholders}) ORDER BY id ASC"
    );
    // `placeholders` is `?1,?2,…` built from a count, not user input — safe.
    let mut q = sqlx::query_as::<_, MsgAttRow>(sqlx::AssertSqlSafe(att_sql));
    for id in &ids {
        q = q.bind(*id);
    }
    let atts = q.fetch_all(pool).await?;

    let mut by_msg: std::collections::HashMap<String, Vec<Attachment>> =
        std::collections::HashMap::new();
    for a in atts {
        by_msg.entry(a.message_id).or_default().push(Attachment {
            id: a.id,
            filename: a.filename,
            content_type: a.content_type,
            size: a.size,
            width: a.width,
            height: a.height,
            has_thumb: a.has_thumb,
        });
    }

    // query was DESC for the LIMIT; hand back oldest-first
    Ok(rows
        .into_iter()
        .rev()
        .map(|r| {
            let reply_to = r.reply_preview();
            ChatMessage {
                attachments: by_msg.remove(&r.id).unwrap_or_default(),
                id: r.id,
                room_id: r.room_id,
                author_id: r.author_id,
                author_name: r.author_name,
                author_avatar: r.author_avatar,
                body: r.body,
                reply_to,
                client_msg_id: r.client_msg_id,
                created_at: r.created_at,
                edited_at: r.edited_at,
            }
        })
        .collect())
}

/// `(room_id, author_id)` for a message, or None if it doesn't exist. Used to
/// authorize an edit/delete before applying it.
pub async fn message_meta(reads: &SqlitePool, id: &str) -> sqlx::Result<Option<(String, String)>> {
    sqlx::query_as::<_, (String, String)>("SELECT room_id, author_id FROM messages WHERE id = ?1")
        .bind(id)
        .fetch_optional(reads)
        .await
}

/// Replace a message body and stamp `edited_at`. Permission is checked by the
/// caller (author only).
pub async fn edit_message(write: &SqlitePool, id: &str, body: &str, ts: i64) -> sqlx::Result<()> {
    sqlx::query("UPDATE messages SET body = ?2, edited_at = ?3 WHERE id = ?1")
        .bind(id)
        .bind(body)
        .bind(ts)
        .execute(write)
        .await?;
    Ok(())
}

/// Delete a message and its attachment rows. Permission is checked by the caller
/// (author, or an admin). Attachment blobs on disk are left for a later GC.
pub async fn delete_message(write: &SqlitePool, id: &str) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM attachments WHERE message_id = ?1")
        .bind(id)
        .execute(write)
        .await?;
    sqlx::query("DELETE FROM messages WHERE id = ?1")
        .bind(id)
        .execute(write)
        .await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct ReplyRow {
    id: String,
    author_name: String,
    body: String,
    has_att: i64,
}

/// Build a reply preview for a quoted message, but only if it exists in the same
/// room (so a reply can't quote across rooms). Used on the live send path; the
/// history path derives the same shape via a self-join.
pub async fn reply_preview(
    reads: &SqlitePool,
    id: &str,
    room_id: &str,
) -> sqlx::Result<Option<ReplyPreview>> {
    let row = sqlx::query_as::<_, ReplyRow>(
        "SELECT m.id, m.author_name, m.body,
                EXISTS(SELECT 1 FROM attachments a WHERE a.message_id = m.id) AS has_att
         FROM messages m WHERE m.id = ?1 AND m.room_id = ?2",
    )
    .bind(id)
    .bind(room_id)
    .fetch_optional(reads)
    .await?;
    Ok(row.map(|r| ReplyPreview {
        id: r.id,
        author_name: r.author_name,
        body: r.body,
        has_attachment: r.has_att != 0,
    }))
}

/// Persist attachment metadata (bytes are written to `Storage` separately).
#[allow(clippy::too_many_arguments)]
pub async fn insert_attachment(
    write: &SqlitePool,
    a: &Attachment,
    room_id: &str,
    uploader_id: &str,
    created_at: i64,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO attachments
           (id, room_id, uploader_id, filename, content_type, size, width, height, has_thumb, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )
    .bind(&a.id)
    .bind(room_id)
    .bind(uploader_id)
    .bind(&a.filename)
    .bind(&a.content_type)
    .bind(a.size)
    .bind(a.width)
    .bind(a.height)
    .bind(a.has_thumb)
    .bind(created_at)
    .execute(write)
    .await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct AttRow {
    room_id: String,
    id: String,
    filename: String,
    content_type: String,
    size: i64,
    width: Option<i64>,
    height: Option<i64>,
    has_thumb: bool,
}

/// Look up an attachment plus the room it belongs to (for access checks).
pub async fn lookup_attachment(
    reads: &SqlitePool,
    id: &str,
) -> sqlx::Result<Option<(String, Attachment)>> {
    let row = sqlx::query_as::<_, AttRow>(
        "SELECT room_id, id, filename, content_type, size, width, height, has_thumb
         FROM attachments WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(reads)
    .await?;
    Ok(row.map(|r| {
        (
            r.room_id,
            Attachment {
                id: r.id,
                filename: r.filename,
                content_type: r.content_type,
                size: r.size,
                width: r.width,
                height: r.height,
                has_thumb: r.has_thumb,
            },
        )
    }))
}

/// A client's room id, if it exists (read-only; no creation).
pub async fn room_of_client(reads: &SqlitePool, client_id: &str) -> sqlx::Result<Option<String>> {
    let row = sqlx::query_as::<_, (String,)>("SELECT id FROM rooms WHERE client_id = ?1")
        .bind(client_id)
        .fetch_optional(reads)
        .await?;
    Ok(row.map(|(id,)| id))
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

/// Whether a principal may access a room: employees see every room; a client
/// only its own. Used to gate both chat joins and call start/join.
pub async fn can_access_room(
    reads: &SqlitePool,
    principal_kind: &str,
    principal_id: &str,
    room_id: &str,
) -> sqlx::Result<bool> {
    if crate::auth::is_staff(principal_kind) {
        return staff_can_open(reads, principal_id, room_id).await;
    }
    Ok(room_of_client(reads, principal_id).await?.as_deref() == Some(room_id))
}

/// Whether an employee may open a room: every common/client room, but a `direct`
/// room only if they're one of its two members (DMs are private).
pub async fn staff_can_open(
    reads: &SqlitePool,
    principal_id: &str,
    room_id: &str,
) -> sqlx::Result<bool> {
    let ok: bool = sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1 FROM rooms r WHERE r.id = ?2
             AND (r.kind <> 'direct'
                  OR EXISTS(SELECT 1 FROM room_members m
                            WHERE m.room_id = r.id AND m.principal_id = ?1)))",
    )
    .bind(principal_id)
    .bind(room_id)
    .fetch_one(reads)
    .await?;
    Ok(ok)
}

/// Find-or-create the 1:1 direct room between two principals. The id is derived
/// from the sorted pair, so calling it from either side returns the same room
/// (idempotent — safe to call freely). Returns the room id.
pub async fn ensure_direct_room(write: &SqlitePool, a: &str, b: &str) -> sqlx::Result<String> {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    let id = format!("dm-{lo}-{hi}");
    sqlx::query("INSERT INTO rooms (id, kind, created_at) VALUES (?1, 'direct', ?2) ON CONFLICT(id) DO NOTHING")
        .bind(&id)
        .bind(crate::now_millis())
        .execute(write)
        .await?;
    for p in [lo, hi] {
        sqlx::query("INSERT INTO room_members (room_id, principal_id) VALUES (?1, ?2) ON CONFLICT DO NOTHING")
            .bind(&id)
            .bind(p)
            .execute(write)
            .await?;
    }
    Ok(id)
}

/// Everyone who should be *notified* about a message in `room_id`: employees for
/// common/client rooms, the client for their own room, and the two members of a
/// direct room. Bots are excluded (they have no devices). The caller still filters
/// out the author and anyone currently online.
pub async fn room_audience(reads: &SqlitePool, room_id: &str) -> sqlx::Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT p.id FROM principals p
         WHERE (p.kind = 'user'
                  AND (SELECT kind FROM rooms WHERE id = ?1) IN ('common', 'client'))
            OR p.id = (SELECT client_id FROM rooms WHERE id = ?1)
            OR EXISTS (SELECT 1 FROM room_members m
                       WHERE m.room_id = ?1 AND m.principal_id = p.id)",
    )
    .bind(room_id)
    .fetch_all(reads)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// Register (or re-point) a device push token. Keyed on the token, so the same
/// device re-registering is idempotent and a device that logs in as someone else
/// just moves to the new principal.
pub async fn upsert_push_token(
    write: &SqlitePool,
    token: &str,
    principal_id: &str,
    platform: &str,
    ts: i64,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO push_tokens (token, principal_id, platform, created_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(token) DO UPDATE SET principal_id = ?2, platform = ?3, created_at = ?4",
    )
    .bind(token)
    .bind(principal_id)
    .bind(platform)
    .bind(ts)
    .execute(write)
    .await?;
    Ok(())
}

/// Drop a push token (on logout, or when FCM reports it unregistered).
pub async fn delete_push_token(write: &SqlitePool, token: &str) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM push_tokens WHERE token = ?1")
        .bind(token)
        .execute(write)
        .await?;
    Ok(())
}

/// All `(token, principal_id)` device tokens for the given principals (for push
/// fan-out). Empty input → empty result (no query).
pub async fn tokens_for_principals(
    reads: &SqlitePool,
    ids: &[String],
) -> sqlx::Result<Vec<(String, String)>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = (1..=ids.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(",");
    // `placeholders` is `?1,?2,…` from a count, not user input — safe.
    let sql = format!("SELECT token, principal_id FROM push_tokens WHERE principal_id IN ({placeholders})");
    let mut q = sqlx::query_as::<_, (String, String)>(sqlx::AssertSqlSafe(sql));
    for id in ids {
        q = q.bind(id);
    }
    q.fetch_all(reads).await
}

/// A principal's kind (`user`/`client`/`bot`), or None if it doesn't exist.
pub async fn principal_kind(reads: &SqlitePool, id: &str) -> sqlx::Result<Option<String>> {
    sqlx::query_scalar("SELECT kind FROM principals WHERE id = ?1")
        .bind(id)
        .fetch_optional(reads)
        .await
}

/// Record the start of a call (Phase 5 later sets `recording_id` + `ended_at`).
pub async fn insert_call(
    write: &SqlitePool,
    id: &str,
    room_id: &str,
    started_by: &str,
    started_at: i64,
) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO calls (id, room_id, started_by, started_at) VALUES (?1, ?2, ?3, ?4)")
        .bind(id)
        .bind(room_id)
        .bind(started_by)
        .bind(started_at)
        .execute(write)
        .await?;
    Ok(())
}

/// Mark a call ended (idempotent — only sets `ended_at` while still NULL).
pub async fn end_call(write: &SqlitePool, id: &str, ended_at: i64) -> sqlx::Result<()> {
    sqlx::query("UPDATE calls SET ended_at = ?2 WHERE id = ?1 AND ended_at IS NULL")
        .bind(id)
        .bind(ended_at)
        .execute(write)
        .await?;
    Ok(())
}

/// Point a call at its server-side recording (Phase 5). The recordings live as
/// `<recording_id>.<participant_id>.ogg` under the recordings dir.
pub async fn set_call_recording(
    write: &SqlitePool,
    id: &str,
    recording_id: &str,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE calls SET recording_id = ?2 WHERE id = ?1")
        .bind(id)
        .bind(recording_id)
        .execute(write)
        .await?;
    Ok(())
}

/// A finished call that has a recording, for the admin recordings page. Per-track
/// files are discovered on disk; this is just the call-level metadata.
#[derive(Clone, Debug, serde::Serialize, sqlx::FromRow)]
pub struct RecordedCall {
    pub call_id: String,
    /// Client room title (the client's name); `None` for the common room.
    pub room_title: Option<String>,
    pub started_by_name: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
}

/// Recorded calls, newest first, with room + caller names resolved.
pub async fn list_recorded_calls(reads: &SqlitePool) -> sqlx::Result<Vec<RecordedCall>> {
    sqlx::query_as::<_, RecordedCall>(
        "SELECT c.id AS call_id,
                rp.display_name AS room_title,
                sb.display_name AS started_by_name,
                c.started_at, c.ended_at
         FROM calls c
         JOIN rooms r ON r.id = c.room_id
         LEFT JOIN principals rp ON rp.id = r.client_id
         LEFT JOIN principals sb ON sb.id = c.started_by
         WHERE c.recording_id IS NOT NULL
         ORDER BY c.started_at DESC",
    )
    .fetch_all(reads)
    .await
}

/// Mark a room read for a principal up to `ts`; never moves the mark backward.
pub async fn mark_read(
    write: &SqlitePool,
    principal_id: &str,
    room_id: &str,
    ts: i64,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO room_reads (principal_id, room_id, last_read_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(principal_id, room_id) DO UPDATE SET last_read_at = MAX(last_read_at, ?3)",
    )
    .bind(principal_id)
    .bind(room_id)
    .bind(ts)
    .execute(write)
    .await?;
    Ok(())
}

/// Whether a principal has any read mark yet — false only on their first ever
/// connect, when we baseline all rooms to "read" so old history isn't unread.
pub async fn has_reads(reads: &SqlitePool, principal_id: &str) -> sqlx::Result<bool> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM room_reads WHERE principal_id = ?1")
        .bind(principal_id)
        .fetch_one(reads)
        .await?;
    Ok(n > 0)
}

/// Unread counts per room for a principal: messages newer than their read mark,
/// excluding their own. A room with no mark counts from the start (a brand-new
/// room is all-unread). Only rooms with a non-zero count are returned.
pub async fn unread_counts(
    reads: &SqlitePool,
    principal_id: &str,
    room_ids: &[String],
) -> sqlx::Result<std::collections::HashMap<String, i64>> {
    let mut out = std::collections::HashMap::new();
    for room in room_ids {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM messages m
             WHERE m.room_id = ?1 AND m.author_id != ?2
               AND m.created_at > COALESCE(
                 (SELECT last_read_at FROM room_reads WHERE principal_id = ?2 AND room_id = ?1), 0)",
        )
        .bind(room)
        .bind(principal_id)
        .fetch_one(reads)
        .await?;
        if n > 0 {
            out.insert(room.clone(), n);
        }
    }
    Ok(out)
}

/// Persisted last-seen times (`principal_id → unix millis`), loaded on startup to
/// seed the in-memory presence registry so a restart doesn't lose the data.
pub async fn load_last_seen(
    reads: &SqlitePool,
) -> sqlx::Result<std::collections::HashMap<String, i64>> {
    let rows: Vec<(String, i64)> = sqlx::query_as("SELECT principal_id, ts FROM last_seen")
        .fetch_all(reads)
        .await?;
    Ok(rows.into_iter().collect())
}

/// Persist the whole last-seen map (small: one row per principal). Called
/// periodically and best-effort, so it survives a restart/redeploy.
pub async fn save_last_seen(
    write: &SqlitePool,
    map: &std::collections::HashMap<String, i64>,
) -> sqlx::Result<()> {
    for (id, ts) in map {
        sqlx::query(
            "INSERT INTO last_seen (principal_id, ts) VALUES (?1, ?2)
             ON CONFLICT(principal_id) DO UPDATE SET ts = ?2",
        )
        .bind(id)
        .bind(ts)
        .execute(write)
        .await?;
    }
    Ok(())
}

/// Everyone in the connections list: human principals (employees + clients),
/// not integration bots. Online status + last-seen are overlaid from presence.
pub async fn list_people(
    reads: &SqlitePool,
) -> sqlx::Result<Vec<(String, String, String, Option<String>)>> {
    sqlx::query_as(
        "SELECT id, display_name, kind, avatar FROM principals WHERE kind IN ('user','client')",
    )
    .fetch_all(reads)
    .await
}

/// Map of principal id → display name (for labeling recording tracks by speaker).
pub async fn all_principal_names(
    reads: &SqlitePool,
) -> sqlx::Result<std::collections::HashMap<String, String>> {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT id, display_name FROM principals")
        .fetch_all(reads)
        .await?;
    Ok(rows.into_iter().collect())
}

/// Rooms an employee can see: common first, then each client room (titled with
/// the client's display name), oldest client first.
pub async fn list_rooms_for_user(
    reads: &SqlitePool,
    principal_id: &str,
) -> sqlx::Result<Vec<RoomSummary>> {
    // Common + every client room (employees see all), plus this principal's own
    // direct rooms (private to their two members). A direct room's title is the
    // OTHER member's name; common is pinned first; the rest sort by latest
    // activity (message or call), newest first, falling back to creation.
    let rooms = sqlx::query_as::<_, RoomSummary>(
        "SELECT r.id, r.kind,
           CASE WHEN r.kind = 'direct'
                THEN (SELECT dp.display_name FROM room_members rm
                      JOIN principals dp ON dp.id = rm.principal_id
                      WHERE rm.room_id = r.id AND rm.principal_id <> ?1 LIMIT 1)
                ELSE cp.display_name END AS title,
           CASE WHEN r.kind = 'direct'
                THEN (SELECT rm.principal_id FROM room_members rm
                      WHERE rm.room_id = r.id AND rm.principal_id <> ?1 LIMIT 1)
                ELSE r.client_id END AS client_id,
           r.created_at
         FROM rooms r
         LEFT JOIN principals cp ON cp.id = r.client_id
         WHERE r.kind IN ('common', 'client')
            OR EXISTS (SELECT 1 FROM room_members rm
                       WHERE rm.room_id = r.id AND rm.principal_id = ?1)
         ORDER BY
           (r.kind = 'common') DESC,
           MAX(
             COALESCE((SELECT MAX(m.created_at) FROM messages m WHERE m.room_id = r.id), 0),
             COALESCE((SELECT MAX(c.started_at) FROM calls c WHERE c.room_id = r.id), 0)
           ) DESC,
           r.created_at DESC, r.id DESC",
    )
    .bind(principal_id)
    .fetch_all(reads)
    .await?;
    Ok(rooms)
}
