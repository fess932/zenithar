//! "Сохранёнки" — a user's private saved-image collection (VK-style). Each item is
//! the user's OWN copy of an image (its own Storage blob), so it outlives the
//! message it was saved from. Items can be saved from a message, uploaded
//! directly (see [`crate::uploads::upload_saved`]), marked public for the owner's
//! profile, sent into a room (copied back to a normal attachment), or deleted.

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use ulid::Ulid;

use crate::auth::{Identity, Principal};
use crate::models::{Attachment, SavedItem};
use crate::routes::origin_ok;
use crate::state::AppState;
use crate::storage::thumb_key;
use crate::{db, now_millis};

/// `GET /api/saved` — the caller's own saved items (newest first).
pub async fn list(State(state): State<AppState>, Identity(p): Identity) -> Response {
    match db::list_saved(&state.reads, &p.id).await {
        Ok(items) => Json(items).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `GET /api/saved/of/:principal_id` — another user's PUBLIC saved items (profile).
pub async fn list_of(
    State(state): State<AppState>,
    Identity(_p): Identity,
    Path(pid): Path<String>,
) -> Response {
    match db::list_saved_public(&state.reads, &pid).await {
        Ok(items) => Json(items).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `POST /api/saved/from/:attachment_id` — copy a message image into your saved
/// collection (its own blob, so it survives the original being deleted).
pub async fn save_from(
    State(state): State<AppState>,
    Identity(p): Identity,
    headers: HeaderMap,
    Path(att_id): Path<String>,
) -> Response {
    if !origin_ok(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    // The source must exist and the caller must be able to see its room.
    let Ok(Some((room_id, src))) = db::lookup_attachment(&state.reads, &att_id).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !db::can_access_room(&state.reads, &p.kind, &p.id, &room_id)
        .await
        .unwrap_or(false)
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let new_id = Ulid::new().to_string();
    if copy_blob(&state, &att_id, &new_id, src.has_thumb).await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let item = SavedItem {
        id: new_id,
        filename: src.filename,
        content_type: src.content_type,
        size: src.size,
        width: src.width,
        height: src.height,
        has_thumb: src.has_thumb,
        public: false,
        created_at: now_millis(),
    };
    match db::insert_saved(&state.db, &item, &p.id).await {
        Ok(()) => Json(item).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize)]
pub struct PublicReq {
    pub public: bool,
}

/// `PATCH /api/saved/:id` — flip an item's public flag (owner only).
pub async fn set_public(
    State(state): State<AppState>,
    Identity(p): Identity,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<PublicReq>,
) -> Response {
    if !origin_ok(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match db::set_saved_public(&state.db, &id, &p.id, body.public).await {
        Ok(true) => StatusCode::OK.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `DELETE /api/saved/:id` — remove a saved item + its blob (owner only).
pub async fn delete(
    State(state): State<AppState>,
    Identity(p): Identity,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !origin_ok(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match db::delete_saved(&state.db, &id, &p.id).await {
        Ok(true) => {
            // Best-effort blob cleanup; the row is already gone.
            let storage = state.storage.clone();
            let id2 = id.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let _ = storage.remove(&id2);
                let _ = storage.remove(&thumb_key(&id2));
            })
            .await;
            StatusCode::OK.into_response()
        }
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize)]
pub struct AttachReq {
    pub room_id: String,
}

/// `POST /api/saved/:id/attach` — copy a saved item into a normal room attachment
/// so it can be sent. Returns the fresh [`Attachment`]; the client then sends it.
pub async fn attach(
    State(state): State<AppState>,
    Identity(p): Identity,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<AttachReq>,
) -> Response {
    if !origin_ok(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(Some((owner, item))) = db::get_saved(&state.reads, &id).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if owner != p.id {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !db::can_access_room(&state.reads, &p.kind, &p.id, &body.room_id)
        .await
        .unwrap_or(false)
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let new_id = Ulid::new().to_string();
    if copy_blob(&state, &id, &new_id, item.has_thumb).await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let att = Attachment {
        id: new_id,
        filename: item.filename,
        content_type: item.content_type,
        size: item.size,
        width: item.width,
        height: item.height,
        has_thumb: item.has_thumb,
    };
    match db::insert_attachment(&state.db, &att, &body.room_id, &p.id, now_millis()).await {
        Ok(()) => Json(att).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `GET /api/saved/:id/file` — original bytes (owner, or anyone if public).
pub async fn serve(
    State(state): State<AppState>,
    Identity(p): Identity,
    Path(id): Path<String>,
) -> Response {
    serve_inner(state, p, &id, false).await
}

/// `GET /api/saved/:id/thumb` — JPEG thumbnail (owner, or anyone if public).
pub async fn serve_thumb(
    State(state): State<AppState>,
    Identity(p): Identity,
    Path(id): Path<String>,
) -> Response {
    serve_inner(state, p, &id, true).await
}

async fn serve_inner(state: AppState, p: Principal, id: &str, thumb: bool) -> Response {
    let Ok(Some((owner, item))) = db::get_saved(&state.reads, id).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if owner != p.id && !item.public {
        return StatusCode::FORBIDDEN.into_response();
    }
    if thumb && !item.has_thumb {
        return StatusCode::NOT_FOUND.into_response();
    }
    let key = if thumb { thumb_key(id) } else { id.to_string() };
    let storage = state.storage.clone();
    let bytes = match tokio::task::spawn_blocking(move || storage.get(&key)).await {
        Ok(Ok(Some(b))) => b,
        Ok(Ok(None)) => return StatusCode::NOT_FOUND.into_response(),
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let content_type = if thumb {
        "image/jpeg".to_string()
    } else {
        item.content_type.clone()
    };
    (
        [
            (header::CONTENT_TYPE, content_type),
            (
                header::CACHE_CONTROL,
                "private, max-age=31536000, immutable".to_string(),
            ),
        ],
        bytes,
    )
        .into_response()
}

/// Copy a Storage blob (and its thumbnail) from `src` key to `dst` key.
async fn copy_blob(state: &AppState, src: &str, dst: &str, has_thumb: bool) -> std::io::Result<()> {
    let storage = state.storage.clone();
    let (src, dst) = (src.to_string(), dst.to_string());
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        let bytes = storage
            .get(&src)?
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "blob gone"))?;
        storage.put(&dst, &bytes)?;
        if has_thumb {
            if let Some(t) = storage.get(&thumb_key(&src))? {
                storage.put(&thumb_key(&dst), &t)?;
            }
        }
        Ok(())
    })
    .await
    .map_err(|_| std::io::Error::other("blocking join failed"))?
}
