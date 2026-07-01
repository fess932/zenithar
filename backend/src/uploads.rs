//! Attachment upload + serving. Bytes go through the `Storage` trait (disk for
//! now). Images get a JPEG thumbnail and recorded dimensions; other files are
//! stored as-is. Access is gated by room membership.

use std::io::Cursor;

use axum::extract::{Multipart, Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use sqlx::SqlitePool;
use ulid::Ulid;

use crate::auth::{Identity, Principal};
use crate::models::{Attachment, SavedItem};
use crate::routes::origin_ok;
use crate::state::AppState;
use crate::storage::{thumb_key, Storage};
use crate::{db, now_millis};

/// Default per-upload ceiling (images, files, voice). Videos get a bigger one.
pub const MAX_UPLOAD_BYTES: usize = 40 * 1024 * 1024;
/// Videos are legitimately large → 200 MB. Also the route body limit (below).
pub const MAX_VIDEO_BYTES: usize = 200 * 1024 * 1024;
const THUMB_MAX: u32 = 320;

/// Per-upload byte ceiling by declared content type: 200 MB for video, else 40 MB.
fn size_limit(declared_ct: &Option<String>) -> usize {
    if declared_ct.as_deref().is_some_and(|ct| ct.starts_with("video/")) {
        MAX_VIDEO_BYTES
    } else {
        MAX_UPLOAD_BYTES
    }
}

/// `POST /api/upload` — multipart `room_id` + `file`, browser path (cookie auth +
/// CSRF origin check). Returns the attachment meta.
pub async fn upload(
    State(state): State<AppState>,
    Identity(p): Identity,
    headers: HeaderMap,
    multipart: Multipart,
) -> Response {
    if !origin_ok(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    ingest(&state, &p, multipart).await
}

/// Shared upload core: rate-limit, parse multipart, validate, store bytes + meta.
/// Reused by the browser route ([`upload`]) and the REST API (Bearer auth).
pub async fn ingest(state: &AppState, p: &Principal, mut multipart: Multipart) -> Response {
    if !state.limits.uploads.check(&p.id) {
        return (StatusCode::TOO_MANY_REQUESTS, "too many uploads").into_response();
    }

    let mut room_id: Option<String> = None;
    let mut filename = String::from("file");
    let mut declared_ct: Option<String> = None;
    let mut bytes: Option<Vec<u8>> = None;

    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(_) => return (StatusCode::BAD_REQUEST, "malformed upload").into_response(),
        };
        match field.name() {
            Some("room_id") => room_id = field.text().await.ok(),
            Some("file") => {
                if let Some(name) = field.file_name() {
                    filename = sanitize_filename(name);
                }
                declared_ct = field.content_type().map(str::to_string);
                match field.bytes().await {
                    Ok(b) => bytes = Some(b.to_vec()),
                    Err(_) => {
                        return (StatusCode::PAYLOAD_TOO_LARGE, "file too large").into_response()
                    }
                }
            }
            _ => {}
        }
    }

    let (Some(room_id), Some(bytes)) = (room_id, bytes) else {
        return (StatusCode::BAD_REQUEST, "room_id and file required").into_response();
    };
    if bytes.is_empty() {
        return (StatusCode::BAD_REQUEST, "empty file").into_response();
    }
    if bytes.len() > size_limit(&declared_ct) {
        return (StatusCode::PAYLOAD_TOO_LARGE, "file too large").into_response();
    }
    if !can_access(&state.reads, p, &room_id).await {
        return StatusCode::FORBIDDEN.into_response();
    }

    let id = Ulid::new().to_string();
    let size = bytes.len() as i64;

    // CPU work (decode + thumbnail) + blocking disk IO off the async runtime.
    let storage = state.storage.clone();
    let id_for_blocking = id.clone();
    let processed = tokio::task::spawn_blocking(move || {
        process_and_store(&*storage, &id_for_blocking, bytes, declared_ct)
    })
    .await;

    let prepared = match processed {
        Ok(Ok(p)) => p,
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let att = Attachment {
        id,
        filename,
        content_type: prepared.content_type,
        size,
        width: prepared.width,
        height: prepared.height,
        has_thumb: prepared.has_thumb,
    };

    if db::insert_attachment(&state.db, &att, &room_id, &p.id, now_millis())
        .await
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    Json(att).into_response()
}

/// `POST /api/saved/upload` — multipart `file`; store an image straight into the
/// caller's saved collection (no room). Returns the [`SavedItem`].
pub async fn upload_saved(
    State(state): State<AppState>,
    Identity(p): Identity,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Response {
    if !origin_ok(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !state.limits.uploads.check(&p.id) {
        return (StatusCode::TOO_MANY_REQUESTS, "too many uploads").into_response();
    }
    let mut filename = String::from("file");
    let mut declared_ct: Option<String> = None;
    let mut bytes: Option<Vec<u8>> = None;
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(_) => return (StatusCode::BAD_REQUEST, "malformed upload").into_response(),
        };
        if field.name() == Some("file") {
            if let Some(name) = field.file_name() {
                filename = sanitize_filename(name);
            }
            declared_ct = field.content_type().map(str::to_string);
            match field.bytes().await {
                Ok(b) => bytes = Some(b.to_vec()),
                Err(_) => return (StatusCode::PAYLOAD_TOO_LARGE, "file too large").into_response(),
            }
        }
    }
    let Some(bytes) = bytes else {
        return (StatusCode::BAD_REQUEST, "file required").into_response();
    };
    if bytes.is_empty() {
        return (StatusCode::BAD_REQUEST, "empty file").into_response();
    }
    if bytes.len() > size_limit(&declared_ct) {
        return (StatusCode::PAYLOAD_TOO_LARGE, "file too large").into_response();
    }

    let id = Ulid::new().to_string();
    let size = bytes.len() as i64;
    let storage = state.storage.clone();
    let id_for_blocking = id.clone();
    let processed = tokio::task::spawn_blocking(move || {
        process_and_store(&*storage, &id_for_blocking, bytes, declared_ct)
    })
    .await;
    let prepared = match processed {
        Ok(Ok(p)) => p,
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let item = SavedItem {
        id,
        filename,
        content_type: prepared.content_type,
        size,
        width: prepared.width,
        height: prepared.height,
        has_thumb: prepared.has_thumb,
        public: false,
        created_at: now_millis(),
    };
    match db::insert_saved(&state.db, &item, &p.id, None).await {
        Ok(()) => Json(item).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

struct Prepared {
    content_type: String,
    width: Option<i64>,
    height: Option<i64>,
    has_thumb: bool,
}

/// Store the original; if it decodes as an image, also store a JPEG thumbnail and
/// record its dimensions. Runs on a blocking thread.
fn process_and_store(
    storage: &dyn Storage,
    id: &str,
    bytes: Vec<u8>,
    declared_ct: Option<String>,
) -> std::io::Result<Prepared> {
    let image = image::load_from_memory(&bytes).ok();
    let prepared = match image {
        Some(img) => {
            let content_type = image::guess_format(&bytes)
                .map(|f| f.to_mime_type().to_string())
                .unwrap_or_else(|_| "image/*".to_string());
            let (width, height) = (img.width() as i64, img.height() as i64);

            // RGB JPEG thumbnail (drops alpha — fine for a preview).
            let thumb =
                image::DynamicImage::ImageRgb8(img.thumbnail(THUMB_MAX, THUMB_MAX).to_rgb8());
            let mut buf = Vec::new();
            let has_thumb = thumb
                .write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Jpeg)
                .is_ok();
            if has_thumb {
                storage.put(&thumb_key(id), &buf)?;
            }
            Prepared {
                content_type,
                width: Some(width),
                height: Some(height),
                has_thumb,
            }
        }
        None => Prepared {
            content_type: sanitize_content_type(declared_ct),
            width: None,
            height: None,
            has_thumb: false,
        },
    };
    storage.put(id, &bytes)?;
    Ok(prepared)
}

/// `GET /api/attachments/:id` — the original bytes (inline). Supports HTTP range
/// requests so video seeks/streams instead of downloading the whole file.
pub async fn serve(
    State(state): State<AppState>,
    Identity(p): Identity,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    serve_inner(state, p, &id, false, range).await
}

/// `GET /api/attachments/:id/thumb` — the JPEG thumbnail (images only).
pub async fn serve_thumb(
    State(state): State<AppState>,
    Identity(p): Identity,
    Path(id): Path<String>,
) -> Response {
    serve_inner(state, p, &id, true, None).await
}

/// Parse a single-range `Range: bytes=…` header into an inclusive `(start, end)`
/// within `total`. Handles `start-`, `start-end`, and `-suffix`; None if bad.
fn parse_range(header: &str, total: u64) -> Option<(u64, u64)> {
    if total == 0 {
        return None;
    }
    let (s, e) = header.trim().strip_prefix("bytes=")?.split_once('-')?;
    let (start, end) = if s.is_empty() {
        (total.saturating_sub(e.parse().ok()?), total - 1)
    } else {
        let start = s.parse().ok()?;
        let end = if e.is_empty() {
            total - 1
        } else {
            e.parse::<u64>().ok()?.min(total - 1)
        };
        (start, end)
    };
    (start <= end && start < total).then_some((start, end))
}

async fn serve_inner(
    state: AppState,
    p: Principal,
    id: &str,
    thumb: bool,
    range: Option<String>,
) -> Response {
    let Ok(Some((room_id, att))) = db::lookup_attachment(&state.reads, id).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !can_access(&state.reads, &p, &room_id).await {
        return StatusCode::FORBIDDEN.into_response();
    }
    if thumb && !att.has_thumb {
        return StatusCode::NOT_FOUND.into_response();
    }

    let key = if thumb { thumb_key(id) } else { id.to_string() };
    let content_type = if thumb {
        "image/jpeg".to_string()
    } else {
        att.content_type.clone()
    };
    let cache = "private, max-age=31536000, immutable".to_string();

    // A range request (video seeking) → 206 with just the requested slice. Thumbs
    // are small images, always served whole.
    if !thumb {
        let storage = state.storage.clone();
        let key2 = key.clone();
        let total = match tokio::task::spawn_blocking(move || storage.size(&key2)).await {
            Ok(Ok(Some(n))) => n,
            Ok(Ok(None)) => return StatusCode::NOT_FOUND.into_response(),
            _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };
        if let Some((start, end)) = range.as_deref().and_then(|r| parse_range(r, total)) {
            let len = end - start + 1;
            let storage = state.storage.clone();
            let key2 = key.clone();
            let bytes =
                match tokio::task::spawn_blocking(move || storage.read_range(&key2, start, len))
                    .await
                {
                    Ok(Ok(Some(b))) => b,
                    _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                };
            return (
                StatusCode::PARTIAL_CONTENT,
                [
                    (header::CONTENT_TYPE, content_type),
                    (header::ACCEPT_RANGES, "bytes".to_string()),
                    (header::CONTENT_RANGE, format!("bytes {start}-{end}/{total}")),
                    (header::CACHE_CONTROL, cache),
                ],
                bytes,
            )
                .into_response();
        }
    }

    let storage = state.storage.clone();
    let bytes = match tokio::task::spawn_blocking(move || storage.get(&key)).await {
        Ok(Ok(Some(b))) => b,
        Ok(Ok(None)) => return StatusCode::NOT_FOUND.into_response(),
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let disposition = format!("inline; filename=\"{}\"", header_safe(&att.filename));
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::ACCEPT_RANGES, "bytes".to_string()),
            (header::CACHE_CONTROL, cache),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        bytes,
    )
        .into_response()
}

/// Employees may access common/client rooms + their own DMs; a client only its
/// own room. (A bot is never a DM member, so DM attachments stay private to the
/// two people.)
async fn can_access(reads: &SqlitePool, p: &Principal, room_id: &str) -> bool {
    db::can_access_room(reads, &p.kind, &p.id, room_id)
        .await
        .unwrap_or(false)
}

/// Display name only (never a path). Strip directories and control chars; cap length.
fn sanitize_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let cleaned: String = base
        .chars()
        .filter(|c| !c.is_control() && *c != '"')
        .take(200)
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "file".to_string()
    } else {
        trimmed.to_string()
    }
}

fn header_safe(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() && *c != '"' && *c != '\\')
        .collect()
}

fn sanitize_content_type(ct: Option<String>) -> String {
    ct.filter(|s| {
        !s.is_empty()
            && s.len() <= 100
            && s.chars().all(|c| c.is_ascii_graphic() || c == ' ')
            && !s.contains(['\r', '\n'])
    })
    .unwrap_or_else(|| "application/octet-stream".to_string())
}
