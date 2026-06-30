//! User avatars. A principal's avatar is either an emoji grapheme or a photo.
//! Emoji are stored inline in `principals.avatar`; photos are square JPEGs in
//! `Storage` under `av_<id>`, with the column set to `"photo:<millis>"` so the
//! client can cache-bust. None → the client draws a default emoji from the id.

use std::io::Cursor;

use axum::extract::{Multipart, Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use serde::{Deserialize, Serialize};

use crate::auth::{self, Identity};
use crate::routes::origin_ok;
use crate::state::AppState;
use crate::now_millis;

/// 6 MB ceiling for an avatar upload (decoded + re-encoded server-side anyway).
const MAX_AVATAR_BYTES: usize = 6 * 1024 * 1024;
/// Stored avatar edge length in pixels (square, center-cropped).
const AVATAR_PX: u32 = 256;

fn av_key(id: &str) -> String {
    format!("av_{id}")
}

#[derive(Serialize)]
struct AvatarResp {
    avatar: Option<String>,
}

#[derive(Deserialize)]
pub struct EmojiReq {
    /// Emoji grapheme to set, or null/empty to clear (restore the default).
    pub value: Option<String>,
}

/// `POST /api/me/avatar` — set an emoji avatar (or clear it). Browser path:
/// cookie identity + CSRF origin check. Clients are anonymous, so users only.
pub async fn set_emoji(
    State(state): State<AppState>,
    Identity(p): Identity,
    headers: HeaderMap,
    Json(body): Json<EmojiReq>,
) -> Response {
    if !origin_ok(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if p.kind != "user" {
        return StatusCode::FORBIDDEN.into_response();
    }

    // Trim; empty → clear. Keep it short and single-line so it can't smuggle a
    // "photo:" sentinel or markup into the column.
    let value = body.value.unwrap_or_default();
    let value = value.trim();
    let next: Option<&str> = if value.is_empty() {
        None
    } else if value.len() > 32 || value.contains(['\r', '\n']) || value.starts_with("photo:") {
        return (StatusCode::BAD_REQUEST, "invalid emoji").into_response();
    } else {
        Some(value)
    };

    match auth::set_avatar(&state.db, &p.id, next).await {
        Ok(()) => Json(AvatarResp {
            avatar: next.map(str::to_string),
        })
        .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `POST /api/me/avatar/photo` — multipart `file`; decoded, square-cropped, and
/// stored as a JPEG. Sets the column to `"photo:<millis>"`.
pub async fn set_photo(
    State(state): State<AppState>,
    Identity(p): Identity,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Response {
    if !origin_ok(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if p.kind != "user" {
        return StatusCode::FORBIDDEN.into_response();
    }

    let mut bytes: Option<Vec<u8>> = None;
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(_) => return (StatusCode::BAD_REQUEST, "malformed upload").into_response(),
        };
        if field.name() == Some("file") {
            match field.bytes().await {
                Ok(b) => bytes = Some(b.to_vec()),
                Err(_) => return (StatusCode::PAYLOAD_TOO_LARGE, "file too large").into_response(),
            }
        }
    }

    let Some(bytes) = bytes else {
        return (StatusCode::BAD_REQUEST, "file required").into_response();
    };
    if bytes.is_empty() || bytes.len() > MAX_AVATAR_BYTES {
        return (StatusCode::BAD_REQUEST, "bad file size").into_response();
    }

    // Decode + crop + re-encode + store off the async runtime. Ok(true) stored,
    // Ok(false) = not a decodable image.
    let storage = state.storage.clone();
    let key = av_key(&p.id);
    let stored = tokio::task::spawn_blocking(move || match square_jpeg(&bytes) {
        Some(jpg) => storage.put(&key, &jpg).map(|()| true),
        None => Ok(false),
    })
    .await;
    match stored {
        Ok(Ok(true)) => {}
        Ok(Ok(false)) => return (StatusCode::UNSUPPORTED_MEDIA_TYPE, "not an image").into_response(),
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }

    let value = format!("photo:{}", now_millis());
    match auth::set_avatar(&state.db, &p.id, Some(&value)).await {
        Ok(()) => Json(AvatarResp { avatar: Some(value) }).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `GET /api/avatars/:id` — the stored avatar JPEG for a principal (photo avatars
/// only). Any signed-in principal may fetch any avatar; they aren't secret within
/// the workspace. Cache-busting is via the `?v=` the client appends.
pub async fn serve(
    State(state): State<AppState>,
    Identity(_): Identity,
    Path(id): Path<String>,
) -> Response {
    // Only serve when the principal actually has a photo avatar.
    match auth::get_avatar(&state.reads, &id).await {
        Ok(Some(v)) if v.starts_with("photo:") => {}
        _ => return StatusCode::NOT_FOUND.into_response(),
    }

    let storage = state.storage.clone();
    let key = av_key(&id);
    let bytes = match tokio::task::spawn_blocking(move || storage.get(&key)).await {
        Ok(Ok(Some(b))) => b,
        Ok(Ok(None)) => return StatusCode::NOT_FOUND.into_response(),
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    (
        [
            (header::CONTENT_TYPE, "image/jpeg".to_string()),
            (
                header::CACHE_CONTROL,
                "private, max-age=31536000, immutable".to_string(),
            ),
        ],
        bytes,
    )
        .into_response()
}

/// Decode any supported image and return a square, center-cropped JPEG. None if
/// the bytes aren't a decodable image.
fn square_jpeg(bytes: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?;
    // resize_to_fill center-crops to exactly fill the square box.
    let square = img.resize_to_fill(AVATAR_PX, AVATAR_PX, image::imageops::FilterType::Lanczos3);
    let mut buf = Vec::new();
    image::DynamicImage::ImageRgb8(square.to_rgb8())
        .write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Jpeg)
        .ok()?;
    Some(buf)
}
