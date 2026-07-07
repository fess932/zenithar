//! Sticker/emoji/gif packs inside "сохранёнки". A pack groups saved_items (each
//! its own Storage blob) under a name and a `kind` (sticker | gif | saved), shown
//! as a separate sub-list. Two ways to fill one: create it empty and POST items,
//! or import a `.wastickers`/`.zip` archive (WhatsApp packs — a ZIP of WebP) or a
//! `.tgs` (Telegram animated sticker — gzipped Lottie JSON) in one shot.
//!
//! Sharing is Telegram-style: every pack has an unguessable `share_slug`. Anyone
//! with the link can preview it (slug-scoped blob reads) and copy the whole pack
//! into their own collection — fresh blobs, so it survives the original's deletion.

use std::io::{Cursor, Read};

use axum::extract::{Multipart, Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use ulid::Ulid;

use crate::auth::Identity;
use crate::models::{PackWithItems, SavedItem, SavedPack};
use crate::routes::origin_ok;
use crate::state::AppState;
use crate::{db, now_millis, uploads};

/// Lottie animations are transparent JSON; give them a distinct content type so
/// the client renders them with the animation player instead of an <img>/file card.
pub const LOTTIE_CT: &str = "application/lottie+json";
/// Cap on how many entries we'll pull out of one imported archive.
const MAX_PACK_ITEMS: usize = 200;

/// Normalise a caller-supplied pack kind to the small allowed set.
fn norm_kind(raw: Option<&str>) -> String {
    match raw {
        Some("sticker") => "sticker",
        Some("gif") => "gif",
        _ => "saved",
    }
    .to_string()
}

/// Infer a pack's kind from what's inside it (used when the caller doesn't force
/// one): any Lottie/WebM/WebP → a sticker pack; else any GIF → a gif pack; else
/// (plain PNG/JPEG photos) → a saved-images pack.
fn detect_kind(entries: &[(String, Vec<u8>)]) -> &'static str {
    let mut has_sticker = false;
    let mut has_gif = false;
    for (name, bytes) in entries {
        if name.to_lowercase().ends_with(".tgs") || is_gzip(bytes) {
            has_sticker = true;
        } else {
            match sniff_media_ct(bytes) {
                Some("image/webp") | Some("video/webm") => has_sticker = true,
                Some("image/gif") => has_gif = true,
                _ => {}
            }
        }
    }
    if has_sticker {
        "sticker"
    } else if has_gif {
        "gif"
    } else {
        "saved"
    }
}

/// `GET /api/packs` — the caller's own packs, each with its member items.
pub async fn list(State(state): State<AppState>, Identity(p): Identity) -> Response {
    let Ok(packs) = db::list_packs(&state.reads, &p.id).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let mut out = Vec::with_capacity(packs.len());
    for pack in packs {
        let items = db::pack_items(&state.reads, &pack.id)
            .await
            .unwrap_or_default();
        out.push(PackWithItems { pack, items });
    }
    Json(out).into_response()
}

/// `GET /api/packs/of/:principal_id` — another user's PUBLIC packs (their profile).
pub async fn list_of(
    State(state): State<AppState>,
    Identity(_p): Identity,
    Path(pid): Path<String>,
) -> Response {
    let Ok(packs) = db::list_packs_public(&state.reads, &pid).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let mut out = Vec::with_capacity(packs.len());
    for pack in packs {
        let items = db::pack_items(&state.reads, &pack.id)
            .await
            .unwrap_or_default();
        out.push(PackWithItems { pack, items });
    }
    Json(out).into_response()
}

#[derive(Deserialize)]
pub struct CreateReq {
    pub name: String,
    pub kind: Option<String>,
}

/// `POST /api/packs` — create an empty pack; items are added via `add_items`.
pub async fn create(
    State(state): State<AppState>,
    Identity(p): Identity,
    headers: HeaderMap,
    Json(body): Json<CreateReq>,
) -> Response {
    if !origin_ok(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let name = body.name.trim();
    if name.is_empty() || name.len() > 80 {
        return (StatusCode::BAD_REQUEST, "bad name").into_response();
    }
    let pack = SavedPack {
        id: Ulid::new().to_string(),
        owner_id: p.id.clone(),
        name: name.to_string(),
        kind: norm_kind(body.kind.as_deref()),
        public: false,
        cover_item_id: None,
        share_slug: Ulid::new().to_string().to_lowercase(),
        created_at: now_millis(),
    };
    match db::insert_pack(&state.db, &pack).await {
        Ok(()) => Json(PackWithItems {
            pack,
            items: Vec::new(),
        })
        .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize)]
pub struct UpdateReq {
    pub name: Option<String>,
    pub kind: Option<String>,
    pub public: Option<bool>,
}

/// `PATCH /api/packs/:id` — rename and/or re-kind a pack (owner only).
pub async fn update(
    State(state): State<AppState>,
    Identity(p): Identity,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<UpdateReq>,
) -> Response {
    if !origin_ok(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut changed = false;
    if let Some(name) = body.name.as_deref().map(str::trim) {
        if name.is_empty() || name.len() > 80 {
            return (StatusCode::BAD_REQUEST, "bad name").into_response();
        }
        match db::rename_pack(&state.db, &id, &p.id, name).await {
            Ok(ok) => changed |= ok,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
    if let Some(kind) = body.kind.as_deref() {
        match db::set_pack_kind(&state.db, &id, &p.id, &norm_kind(Some(kind))).await {
            Ok(ok) => changed |= ok,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
    if let Some(public) = body.public {
        match db::set_pack_public(&state.db, &id, &p.id, public).await {
            Ok(ok) => changed |= ok,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
    if changed {
        StatusCode::OK.into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

/// `DELETE /api/packs/:id` — remove the pack, its items, and their blobs (owner).
pub async fn delete(
    State(state): State<AppState>,
    Identity(p): Identity,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !origin_ok(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match db::delete_pack(&state.db, &id, &p.id).await {
        Ok(ids) if !ids.is_empty() => {
            remove_blobs(&state, ids).await;
            StatusCode::OK.into_response()
        }
        // Empty vec = pack didn't exist or wasn't owned; treat as gone either way.
        Ok(_) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `DELETE /api/packs/:id/items/:item_id` — drop one item + its blob (owner).
pub async fn delete_item(
    State(state): State<AppState>,
    Identity(p): Identity,
    headers: HeaderMap,
    Path((pack_id, item_id)): Path<(String, String)>,
) -> Response {
    if !origin_ok(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match db::delete_pack_item(&state.db, &pack_id, &item_id, &p.id).await {
        Ok(true) => {
            remove_blobs(&state, vec![item_id]).await;
            StatusCode::OK.into_response()
        }
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `POST /api/packs/:id/items` — multipart `file`; add one image/`.tgs` to a pack
/// the caller owns. Returns the new [`SavedItem`].
pub async fn add_items(
    State(state): State<AppState>,
    Identity(p): Identity,
    headers: HeaderMap,
    Path(id): Path<String>,
    mut multipart: Multipart,
) -> Response {
    if !origin_ok(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !state.limits.uploads.check(&p.id) {
        return (StatusCode::TOO_MANY_REQUESTS, "too many uploads").into_response();
    }
    // Must own the pack; its kind decides whether items render bare (sticker/gif).
    let sticker = match db::get_pack(&state.reads, &id).await {
        Ok(Some(pack)) if pack.owner_id == p.id => pack.kind != "saved",
        Ok(_) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let (filename, bytes) = match read_file_field(&mut multipart).await {
        Ok(Some(f)) => f,
        Ok(None) => return (StatusCode::BAD_REQUEST, "file required").into_response(),
        Err(resp) => return resp,
    };
    match store_item(&state, &p.id, &id, &filename, bytes, sticker).await {
        Some(item) => {
            // First item becomes the cover.
            if db::get_pack(&state.reads, &id)
                .await
                .ok()
                .flatten()
                .and_then(|pk| pk.cover_item_id)
                .is_none()
            {
                let _ = db::set_pack_cover(&state.db, &id, &item.id).await;
            }
            Json(item).into_response()
        }
        None => StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response(),
    }
}

/// `POST /api/packs/import` — multipart `file` (a `.wastickers`/`.zip` archive or a
/// single `.tgs`) + optional `name`/`kind`. Creates a pack and fills it.
pub async fn import(
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
    let mut name: Option<String> = None;
    let mut kind: Option<String> = None;
    let mut file: Option<(String, Vec<u8>)> = None;
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(_) => return (StatusCode::BAD_REQUEST, "malformed upload").into_response(),
        };
        match field.name() {
            Some("name") => name = field.text().await.ok(),
            Some("kind") => kind = field.text().await.ok(),
            Some("file") => {
                let fname = field.file_name().unwrap_or("pack").to_string();
                match field.bytes().await {
                    Ok(b) => file = Some((fname, b.to_vec())),
                    Err(_) => {
                        return (StatusCode::PAYLOAD_TOO_LARGE, "file too large").into_response()
                    }
                }
            }
            _ => {}
        }
    }
    let Some((fname, bytes)) = file else {
        return (StatusCode::BAD_REQUEST, "file required").into_response();
    };
    // Split the archive (or single file) into (name, bytes) entries.
    let entries = match unpack(&fname, bytes) {
        Some(e) if !e.is_empty() => e,
        _ => return (StatusCode::UNSUPPORTED_MEDIA_TYPE, "unrecognised pack").into_response(),
    };
    // Default the pack name to the archive's base filename.
    let pack_name = name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fname.rsplit('/').next().unwrap_or("pack"))
        .chars()
        .take(80)
        .collect::<String>();
    // Kind: honour an explicit override, otherwise infer it from what's inside the
    // archive (.tgs/.webm/.webp → stickers, .gif → gifs, plain photos → saved).
    let kind = match kind.as_deref().map(str::trim) {
        Some(k) if !k.is_empty() => norm_kind(Some(k)),
        _ => detect_kind(&entries).to_string(),
    };
    let pack = SavedPack {
        id: Ulid::new().to_string(),
        owner_id: p.id.clone(),
        name: pack_name,
        kind,
        public: false,
        cover_item_id: None,
        share_slug: Ulid::new().to_string().to_lowercase(),
        created_at: now_millis(),
    };
    if db::insert_pack(&state.db, &pack).await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    // A sticker/gif pack's items render bare; a "saved" pack keeps photo framing.
    let sticker = pack.kind != "saved";
    let mut items = Vec::new();
    for (entry_name, entry_bytes) in entries.into_iter().take(MAX_PACK_ITEMS) {
        if let Some(item) =
            store_item(&state, &p.id, &pack.id, &entry_name, entry_bytes, sticker).await
        {
            items.push(item);
        }
    }
    if items.is_empty() {
        // Nothing decoded — roll the empty pack back so it doesn't linger.
        let _ = db::delete_pack(&state.db, &pack.id, &p.id).await;
        return (StatusCode::UNSUPPORTED_MEDIA_TYPE, "no usable stickers").into_response();
    }
    let _ = db::set_pack_cover(&state.db, &pack.id, &items[0].id).await;
    Json(PackWithItems { pack, items }).into_response()
}

/// `GET /api/packs/:slug/preview` — public pack view (anyone with the link).
pub async fn preview(State(state): State<AppState>, Path(slug): Path<String>) -> Response {
    let Ok(Some(pack)) = db::get_pack_by_slug(&state.reads, &slug).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let items = db::pack_items(&state.reads, &pack.id)
        .await
        .unwrap_or_default();
    Json(PackWithItems { pack, items }).into_response()
}

/// `GET /api/packs/:slug/items/:item_id/file` — a shared pack's blob. Knowing the
/// slug grants read (so a not-yet-added pack still renders its previews).
pub async fn serve_shared_item(
    State(state): State<AppState>,
    Path((slug, item_id)): Path<(String, String)>,
) -> Response {
    let Ok(Some(pack)) = db::get_pack_by_slug(&state.reads, &slug).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(Some((_owner, item))) = db::get_saved(&state.reads, &item_id).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // The item must actually belong to this pack (checked via its members list).
    let belongs = db::pack_items(&state.reads, &pack.id)
        .await
        .map(|its| its.iter().any(|i| i.id == item_id))
        .unwrap_or(false);
    if !belongs {
        return StatusCode::NOT_FOUND.into_response();
    }
    let key = item_id.clone();
    let storage = state.storage.clone();
    let bytes = match tokio::task::spawn_blocking(move || storage.get(&key)).await {
        Ok(Ok(Some(b))) => b,
        Ok(Ok(None)) => return StatusCode::NOT_FOUND.into_response(),
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    (
        [
            (header::CONTENT_TYPE, item.content_type),
            (
                header::CACHE_CONTROL,
                "private, max-age=31536000, immutable".to_string(),
            ),
        ],
        bytes,
    )
        .into_response()
}

/// `POST /api/packs/:slug/add` — copy a shared pack (all its blobs) into the
/// caller's own collection. Returns the fresh [`PackWithItems`].
pub async fn add_from_slug(
    State(state): State<AppState>,
    Identity(p): Identity,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Response {
    if !origin_ok(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(Some(src_pack)) = db::get_pack_by_slug(&state.reads, &slug).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let src_items = db::pack_items(&state.reads, &src_pack.id)
        .await
        .unwrap_or_default();
    let new_pack = SavedPack {
        id: Ulid::new().to_string(),
        owner_id: p.id.clone(),
        name: src_pack.name.clone(),
        kind: src_pack.kind.clone(),
        public: false,
        cover_item_id: None,
        share_slug: Ulid::new().to_string().to_lowercase(),
        created_at: now_millis(),
    };
    if db::insert_pack(&state.db, &new_pack).await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let mut items = Vec::new();
    for src in src_items {
        let new_id = Ulid::new().to_string();
        if crate::saved::copy_blob(&state, &src.id, &new_id, src.has_thumb)
            .await
            .is_err()
        {
            continue;
        }
        let item = SavedItem {
            id: new_id,
            created_at: now_millis(),
            ..src
        };
        if db::insert_saved(&state.db, &item, &p.id, None, Some(&new_pack.id))
            .await
            .is_ok()
        {
            items.push(item);
        }
    }
    if let Some(first) = items.first() {
        let _ = db::set_pack_cover(&state.db, &new_pack.id, &first.id).await;
    }
    Json(PackWithItems {
        pack: new_pack,
        items,
    })
    .into_response()
}

// ---- helpers ---------------------------------------------------------------

/// Pull the single `file` field's (filename, bytes) out of a multipart body.
async fn read_file_field(multipart: &mut Multipart) -> Result<Option<(String, Vec<u8>)>, Response> {
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => return Ok(None),
            Err(_) => return Err((StatusCode::BAD_REQUEST, "malformed upload").into_response()),
        };
        if field.name() == Some("file") {
            let fname = field.file_name().unwrap_or("file").to_string();
            return match field.bytes().await {
                Ok(b) => Ok(Some((fname, b.to_vec()))),
                Err(_) => Err((StatusCode::PAYLOAD_TOO_LARGE, "file too large").into_response()),
            };
        }
    }
}

/// Store one pack member: gunzip `.tgs` into a Lottie blob, otherwise run the
/// normal image pipeline (thumbnail + dimensions) while keeping the original as-is
/// so animated WebP/GIF/WebM play. Inserts the row with `pack_id` set. None if the
/// bytes aren't a valid Lottie or a recognised sticker media type.
async fn store_item(
    state: &AppState,
    owner_id: &str,
    pack_id: &str,
    filename: &str,
    bytes: Vec<u8>,
    sticker: bool,
) -> Option<SavedItem> {
    if bytes.is_empty() || bytes.len() > uploads::MAX_UPLOAD_BYTES {
        return None;
    }
    let id = Ulid::new().to_string();
    let is_tgs = filename.to_lowercase().ends_with(".tgs") || is_gzip(&bytes);

    let item = if is_tgs {
        let (json, w, h) = decode_tgs(&bytes)?;
        let size = json.len() as i64;
        let storage = state.storage.clone();
        let key = id.clone();
        tokio::task::spawn_blocking(move || storage.put(&key, &json))
            .await
            .ok()?
            .ok()?;
        SavedItem {
            id,
            filename: filename.to_string(),
            content_type: LOTTIE_CT.to_string(),
            size,
            width: w,
            height: h,
            has_thumb: false,
            has_alpha: true, // Lottie renders transparent — frame it like a sticker.
            is_sticker: sticker,
            public: false,
            created_at: now_millis(),
        }
    } else {
        // Accept any recognised sticker media by its magic bytes (this also filters
        // out a zip's manifest/notes). We keep and serve the ORIGINAL untouched, so
        // animated WebP/GIF animate in an <img> and WebM plays in a <video> — no
        // re-encoding. The image pipeline still runs for a thumbnail + dimensions
        // where it can, but a decode failure (animated WebP first-frame-only, WebM)
        // is fine: we never reject on it.
        let ct = sniff_media_ct(&bytes)?;
        let size = bytes.len() as i64;
        let storage = state.storage.clone();
        let key = id.clone();
        let prepared = tokio::task::spawn_blocking(move || {
            uploads::process_and_store(&*storage, &key, bytes, Some(ct.to_string()))
        })
        .await
        .ok()?
        .ok()?;
        SavedItem {
            id,
            filename: filename.to_string(),
            content_type: ct.to_string(),
            size,
            width: prepared.width,
            height: prepared.height,
            has_thumb: prepared.has_thumb,
            // Stickers render bare (frameless) and straight from the original blob
            // so animation plays — that's exactly what the `has_alpha` path does.
            // (Videos render via <video>, so leave their flag off.)
            has_alpha: ct != "video/webm",
            is_sticker: sticker,
            public: false,
            created_at: now_millis(),
        }
    };
    db::insert_saved(&state.db, &item, owner_id, None, Some(pack_id))
        .await
        .ok()?;
    Some(item)
}

/// Split an uploaded pack file into `(name, bytes)` entries: unzip a ZIP/
/// `.wastickers`, or treat a bare `.tgs`/image as a one-entry pack.
fn unpack(filename: &str, bytes: Vec<u8>) -> Option<Vec<(String, Vec<u8>)>> {
    if is_zip(&bytes) {
        let mut archive = zip::ZipArchive::new(Cursor::new(&bytes)).ok()?;
        let mut out = Vec::new();
        for i in 0..archive.len() {
            let mut f = archive.by_index(i).ok()?;
            if !f.is_file() {
                continue;
            }
            let name = f.name().to_string();
            let lower = name.to_lowercase();
            // Skip the WhatsApp manifest / trays / notes — only real stickers.
            if lower.ends_with(".json") || lower.ends_with(".txt") || lower.contains("tray") {
                continue;
            }
            if !(lower.ends_with(".webp")
                || lower.ends_with(".png")
                || lower.ends_with(".gif")
                || lower.ends_with(".jpg")
                || lower.ends_with(".jpeg")
                || lower.ends_with(".webm")
                || lower.ends_with(".tgs"))
            {
                continue;
            }
            let mut buf = Vec::new();
            if f.read_to_end(&mut buf).is_ok() && !buf.is_empty() {
                out.push((name, buf));
            }
        }
        Some(out)
    } else {
        // A single .tgs / image is a one-item pack.
        Some(vec![(filename.to_string(), bytes)])
    }
}

/// gunzip a `.tgs` to Lottie JSON and read its canvas dimensions (`w`/`h`).
fn decode_tgs(bytes: &[u8]) -> Option<(Vec<u8>, Option<i64>, Option<i64>)> {
    let mut json = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_end(&mut json)
        .ok()?;
    // Must parse as an object with a numeric width to count as Lottie.
    let v: serde_json::Value = serde_json::from_slice(&json).ok()?;
    let w = v.get("w").and_then(|x| x.as_i64());
    let h = v.get("h").and_then(|x| x.as_i64());
    w?;
    Some((json, w, h))
}

/// Identify sticker media by magic bytes → its content type, or None to reject
/// (e.g. a zip's `contents.json`/`.txt`). Covers static + animated WebP/GIF/PNG,
/// JPEG, and WebM video stickers — all stored and served as-is.
fn sniff_media_ct(b: &[u8]) -> Option<&'static str> {
    if b.len() >= 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("image/webp")
    } else if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else if b.starts_with(b"GIF8") {
        Some("image/gif")
    } else if b.starts_with(&[0xff, 0xd8]) {
        Some("image/jpeg")
    } else if b.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        Some("video/webm")
    } else {
        None
    }
}

fn is_gzip(b: &[u8]) -> bool {
    b.len() >= 2 && b[0] == 0x1f && b[1] == 0x8b
}

fn is_zip(b: &[u8]) -> bool {
    b.len() >= 4 && &b[0..4] == b"PK\x03\x04"
}

/// Best-effort removal of item blobs (main + thumbnail) off the request path.
async fn remove_blobs(state: &AppState, ids: Vec<String>) {
    let storage = state.storage.clone();
    let _ = tokio::task::spawn_blocking(move || {
        for id in ids {
            let _ = storage.remove(&id);
            let _ = storage.remove(&crate::storage::thumb_key(&id));
            let _ = storage.remove(&crate::storage::preview_key(&id));
        }
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(bytes).unwrap();
        enc.finish().unwrap()
    }

    #[test]
    fn tgs_decodes_to_lottie_with_dimensions() {
        let lottie = br#"{"v":"5.5.2","w":512,"h":512,"fr":60,"layers":[]}"#;
        let tgs = gzip(lottie);
        assert!(is_gzip(&tgs));
        let (json, w, h) = decode_tgs(&tgs).expect("valid tgs");
        assert_eq!(w, Some(512));
        assert_eq!(h, Some(512));
        assert_eq!(json, lottie);
    }

    #[test]
    fn non_lottie_gzip_is_rejected() {
        // Gzipped JSON without a `w` field isn't a Lottie sticker.
        let tgs = gzip(br#"{"hello":"world"}"#);
        assert!(decode_tgs(&tgs).is_none());
        // Plain (non-gzip) bytes never parse as tgs.
        assert!(decode_tgs(b"not gzip at all").is_none());
    }

    #[test]
    fn format_magic_detection() {
        assert!(is_zip(b"PK\x03\x04rest"));
        assert!(!is_zip(b"\x1f\x8bnope"));
        assert!(is_gzip(b"\x1f\x8b\x08"));
        assert!(!is_gzip(b"PK\x03\x04"));
    }

    #[test]
    fn norm_kind_clamps_to_allowed_set() {
        assert_eq!(norm_kind(Some("sticker")), "sticker");
        assert_eq!(norm_kind(Some("gif")), "gif");
        assert_eq!(norm_kind(Some("emoji")), "saved"); // not yet supported
        assert_eq!(norm_kind(None), "saved");
    }

    #[test]
    fn sniffs_sticker_media_and_rejects_junk() {
        let webp = [b"RIFF", &[0, 0, 0, 0][..], b"WEBP", b"VP8 "].concat();
        assert_eq!(sniff_media_ct(&webp), Some("image/webp"));
        assert_eq!(sniff_media_ct(b"GIF89a..."), Some("image/gif"));
        assert_eq!(
            sniff_media_ct(&[0x89, b'P', b'N', b'G', 13, 10]),
            Some("image/png")
        );
        assert_eq!(sniff_media_ct(&[0xff, 0xd8, 0xff]), Some("image/jpeg"));
        assert_eq!(
            sniff_media_ct(&[0x1a, 0x45, 0xdf, 0xa3]),
            Some("video/webm")
        );
        // A WhatsApp manifest / notes file is not media → rejected.
        assert_eq!(sniff_media_ct(br#"{"identifier":"x"}"#), None);
    }

    #[test]
    fn detect_kind_infers_from_contents() {
        let webp = [b"RIFF", &[0u8; 4][..], b"WEBP"].concat();
        let gif = b"GIF89a".to_vec();
        let png = [0x89, b'P', b'N', b'G', 13, 10].to_vec();
        // Any sticker-ish member wins over gifs/photos.
        assert_eq!(
            detect_kind(&[
                ("a.webp".into(), webp.clone()),
                ("b.gif".into(), gif.clone())
            ]),
            "sticker"
        );
        assert_eq!(
            detect_kind(&[("x.tgs".into(), gzip(br#"{"w":1}"#))]),
            "sticker"
        );
        assert_eq!(detect_kind(&[("only.gif".into(), gif)]), "gif");
        assert_eq!(detect_kind(&[("photo.png".into(), png)]), "saved");
    }

    #[test]
    fn bare_tgs_unpacks_as_single_entry() {
        let entries = unpack("cat.tgs", gzip(br#"{"w":1}"#)).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "cat.tgs");
    }
}
