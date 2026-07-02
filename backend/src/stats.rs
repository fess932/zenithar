//! Admin usage dashboard: a single `/api/admin/stats` snapshot of who/what/how
//! much — principals, rooms, messages, media (with a by-kind byte breakdown),
//! reactions, calls — plus footprint: DB size, real disk usage of the blob store,
//! and the server process's resident memory. All read-only aggregate queries on
//! the reader pool; cheap enough to compute on demand (no caching yet).

use axum::{extract::State, Json};
use serde::Serialize;

use crate::auth::Admin;
use crate::now_millis;
use crate::state::AppState;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Serialize, Default)]
pub struct Stats {
    /// When this snapshot was taken (unix millis) — the client shows "as of …".
    pub generated_at: i64,
    pub principals: Principals,
    pub rooms: Rooms,
    pub messages: Messages,
    pub reactions: i64,
    pub calls: Calls,
    /// Call recordings on disk (separate `recordings/` dir, not the blob store).
    pub recordings: Recordings,
    /// Files sent in chats (originals; thumbnails/orphans counted in storage).
    pub attachments: Media,
    /// Private "сохранёнки" copies.
    pub saved: Media,
    pub storage: StorageInfo,
    /// Server process memory (Linux only; None elsewhere).
    pub memory: Option<Memory>,
}

#[derive(Serialize, Default)]
pub struct Principals {
    pub total: i64,
    pub users: i64,
    pub clients: i64,
    pub bots: i64,
    pub admins: i64,
}

#[derive(Serialize, Default)]
pub struct Rooms {
    pub total: i64,
    pub common: i64,
    pub client: i64,
    pub direct: i64,
}

#[derive(Serialize, Default)]
pub struct Messages {
    pub total: i64,
    pub last_24h: i64,
    pub last_7d: i64,
}

#[derive(Serialize, Default)]
pub struct Calls {
    pub total: i64,
    pub recorded: i64,
}

/// Recorded-call audio files (Ogg/Opus) sitting in the recordings dir.
#[derive(Serialize, Default)]
pub struct Recordings {
    pub count: i64,
    pub bytes: i64,
}

/// A media bucket: how many files and how many bytes, split by kind so the
/// dashboard can show what's eating the space (video usually dominates).
#[derive(Serialize, Default)]
pub struct Media {
    pub count: i64,
    pub bytes: i64,
    pub image_bytes: i64,
    pub video_bytes: i64,
    pub audio_bytes: i64,
    pub other_bytes: i64,
}

#[derive(Serialize, Default)]
pub struct StorageInfo {
    /// SQLite logical size (page_count × page_size), in bytes.
    pub db_bytes: i64,
    /// Real bytes on disk in the blob store (originals + thumbs + orphans) plus
    /// recordings. None if the backend can't enumerate; UI falls back to sums.
    pub blobs_bytes: Option<i64>,
    /// Capacity of the filesystem holding the data dir (the docker-mounted
    /// volume), in bytes. None on non-unix. `fs_avail` is space free to us.
    pub fs_total: Option<i64>,
    pub fs_avail: Option<i64>,
}

#[derive(Serialize, Default)]
pub struct Memory {
    /// Resident set size of the server process, in bytes.
    pub rss_bytes: i64,
}

pub async fn stats(State(state): State<AppState>, _admin: Admin) -> Json<Stats> {
    let now = now_millis();
    let pool = &state.reads;

    let mut out = Stats {
        generated_at: now,
        ..Default::default()
    };

    // principals by kind
    if let Ok(row) = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
        "SELECT COUNT(*), \
         COALESCE(SUM(kind = 'user'), 0), \
         COALESCE(SUM(kind = 'client'), 0), \
         COALESCE(SUM(kind = 'bot'), 0), \
         COALESCE(SUM(is_admin = 1), 0) \
         FROM principals",
    )
    .fetch_one(pool)
    .await
    {
        out.principals = Principals {
            total: row.0,
            users: row.1,
            clients: row.2,
            bots: row.3,
            admins: row.4,
        };
    }

    // rooms by kind
    if let Ok(row) = sqlx::query_as::<_, (i64, i64, i64, i64)>(
        "SELECT COUNT(*), \
         COALESCE(SUM(kind = 'common'), 0), \
         COALESCE(SUM(kind = 'client'), 0), \
         COALESCE(SUM(kind = 'direct'), 0) \
         FROM rooms",
    )
    .fetch_one(pool)
    .await
    {
        out.rooms = Rooms {
            total: row.0,
            common: row.1,
            client: row.2,
            direct: row.3,
        };
    }

    // messages: total + recent windows
    if let Ok(row) = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT COUNT(*), \
         COALESCE(SUM(created_at >= ?1), 0), \
         COALESCE(SUM(created_at >= ?2), 0) \
         FROM messages",
    )
    .bind(now - DAY_MS)
    .bind(now - 7 * DAY_MS)
    .fetch_one(pool)
    .await
    {
        out.messages = Messages {
            total: row.0,
            last_24h: row.1,
            last_7d: row.2,
        };
    }

    out.reactions = scalar(pool, "SELECT COUNT(*) FROM reactions").await;

    if let Ok(row) = sqlx::query_as::<_, (i64, i64)>(
        "SELECT COUNT(*), COALESCE(SUM(recording_id IS NOT NULL), 0) FROM calls",
    )
    .fetch_one(pool)
    .await
    {
        out.calls = Calls {
            total: row.0,
            recorded: row.1,
        };
    }

    out.attachments = media(pool, "attachments").await;
    out.saved = media(pool, "saved_items").await;

    // Call recordings live in their own dir (read via std::fs, not the Storage
    // trait), so measure it directly and fold its bytes into the on-disk total.
    let (rec_count, rec_bytes) = dir_usage(&state.recordings_dir);
    out.recordings = Recordings {
        count: rec_count,
        bytes: rec_bytes,
    };

    // DB logical size = pages × page size.
    let pages = scalar(pool, "PRAGMA page_count").await;
    let page_size = scalar(pool, "PRAGMA page_size").await;
    let (fs_total, fs_avail) = match fs_stats(&state.recordings_dir) {
        Some((total, avail)) => (Some(total), Some(avail)),
        None => (None, None),
    };
    out.storage = StorageInfo {
        db_bytes: pages * page_size,
        // Total on-disk footprint = attachment blobs + recordings.
        blobs_bytes: state
            .storage
            .disk_usage()
            .ok()
            .flatten()
            .map(|b| b as i64 + rec_bytes),
        fs_total,
        fs_avail,
    };

    out.memory = rss_bytes().map(|b| Memory { rss_bytes: b as i64 });

    Json(out)
}

/// One aggregate over a media table, split by content-type family. `table` is a
/// trusted literal ("attachments" | "saved_items"), never user input.
async fn media(pool: &sqlx::SqlitePool, table: &str) -> Media {
    let sql = format!(
        "SELECT COUNT(*), COALESCE(SUM(size), 0), \
         COALESCE(SUM(CASE WHEN content_type LIKE 'image/%' THEN size ELSE 0 END), 0), \
         COALESCE(SUM(CASE WHEN content_type LIKE 'video/%' THEN size ELSE 0 END), 0), \
         COALESCE(SUM(CASE WHEN content_type LIKE 'audio/%' THEN size ELSE 0 END), 0) \
         FROM {table}"
    );
    match sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(sqlx::AssertSqlSafe(sql))
        .fetch_one(pool)
        .await
    {
        Ok((count, bytes, image_bytes, video_bytes, audio_bytes)) => Media {
            count,
            bytes,
            image_bytes,
            video_bytes,
            audio_bytes,
            other_bytes: (bytes - image_bytes - video_bytes - audio_bytes).max(0),
        },
        Err(_) => Media::default(),
    }
}

/// (file count, total bytes) of a flat directory; (0, 0) if it can't be read.
fn dir_usage(dir: &std::path::Path) -> (i64, i64) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (0, 0);
    };
    let mut count = 0i64;
    let mut bytes = 0i64;
    for entry in entries.flatten() {
        if let Ok(m) = entry.metadata() {
            if m.is_file() {
                count += 1;
                bytes += m.len() as i64;
            }
        }
    }
    (count, bytes)
}

/// (total, available) bytes of the filesystem that `path` lives on, via
/// `statvfs`. With a docker bind-mount this reports the underlying host volume —
/// exactly the "space left on the data disk" figure we want. None on non-unix.
#[cfg(unix)]
fn fs_stats(path: &std::path::Path) -> Option<(i64, i64)> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: zeroed statvfs is a valid init state; c_path is a valid NUL-
    // terminated path pointer living for the call.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    // f_frsize is the fundamental block size; fall back to f_bsize if it's 0.
    let unit = if stat.f_frsize != 0 {
        stat.f_frsize
    } else {
        stat.f_bsize
    } as i64;
    let total = stat.f_blocks as i64 * unit;
    // f_bavail = blocks free to unprivileged users (what we can actually use).
    let avail = stat.f_bavail as i64 * unit;
    Some((total, avail))
}

#[cfg(not(unix))]
fn fs_stats(_path: &std::path::Path) -> Option<(i64, i64)> {
    None
}

/// Fetch a single i64 (COUNT / PRAGMA) from a trusted literal; 0 on any error so
/// one bad query doesn't sink the whole snapshot.
async fn scalar(pool: &sqlx::SqlitePool, sql: &'static str) -> i64 {
    sqlx::query_scalar::<_, i64>(sql)
        .fetch_one(pool)
        .await
        .unwrap_or(0)
}

/// Resident memory of this process. Linux reads /proc/self/statm (field 2 =
/// resident pages); other platforms report nothing rather than guess.
#[cfg(target_os = "linux")]
fn rss_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let resident_pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    // Linux page size is 4 KiB on x86_64/aarch64 (the deploy targets).
    Some(resident_pages * 4096)
}

#[cfg(not(target_os = "linux"))]
fn rss_bytes() -> Option<u64> {
    None
}
