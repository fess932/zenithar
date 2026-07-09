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
use crate::storage::{preview_key, thumb_key, Storage};
use crate::{db, now_millis};

/// Default per-upload ceiling (images, files, voice). Videos get a bigger one.
pub const MAX_UPLOAD_BYTES: usize = 40 * 1024 * 1024;
/// Videos are legitimately large → 200 MB. Also the route body limit (below).
pub const MAX_VIDEO_BYTES: usize = 200 * 1024 * 1024;
const THUMB_MAX: u32 = 320;
/// Longest-side cap for the in-app viewer preview (a downscaled WebP served
/// instead of a multi-megapixel original when someone taps a photo open).
const PREVIEW_MAX: u32 = 1600;
/// WebP quality for that preview (0–100). ~80 is near-visually-lossless at a
/// fraction of the original's weight.
const PREVIEW_QUALITY: f32 = 80.0;

/// Per-upload byte ceiling by declared content type: 200 MB for video, else 40 MB.
fn size_limit(declared_ct: &Option<String>) -> usize {
    if declared_ct
        .as_deref()
        .is_some_and(|ct| ct.starts_with("video/"))
    {
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
        has_alpha: prepared.has_alpha,
        is_sticker: false,
        pack_slug: None,
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
        has_alpha: prepared.has_alpha,
        is_sticker: false,
        public: false,
        created_at: now_millis(),
    };
    match db::insert_saved(&state.db, &item, &p.id, None, None).await {
        Ok(()) => Json(item).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) struct Prepared {
    pub(crate) content_type: String,
    pub(crate) width: Option<i64>,
    pub(crate) height: Option<i64>,
    pub(crate) has_thumb: bool,
    pub(crate) has_alpha: bool,
}

/// Decode `bytes`, baking in any EXIF orientation so the pixels match how a
/// browser displays the original (portrait phone photos come out upright instead
/// of sideways). Returns None if the bytes aren't a decodable image.
fn decode_oriented(bytes: &[u8]) -> Option<image::DynamicImage> {
    use image::{DynamicImage, ImageDecoder, ImageReader};
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut decoder = reader.into_decoder().ok()?;
    // Read the EXIF orientation before consuming the decoder, then apply it.
    let orientation = decoder.orientation().ok()?;
    let mut img = DynamicImage::from_decoder(decoder).ok()?;
    img.apply_orientation(orientation);
    Some(img)
}

/// Downscale `img` so its longest side is at most `max`, preserving aspect, using
/// SIMD Lanczos3 (fast_image_resize) — several× faster than the image crate's
/// resize at the same quality. Normalizes to 8-bit RGB/RGBA first so the pixel
/// type is one the resizer handles. None if the resize fails.
fn downscale(img: &image::DynamicImage, max: u32) -> Option<image::DynamicImage> {
    use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
    let ratio = max as f32 / img.width().max(img.height()) as f32;
    let dw = ((img.width() as f32 * ratio).round() as u32).max(1);
    let dh = ((img.height() as f32 * ratio).round() as u32).max(1);
    let src: image::DynamicImage = if img.color().has_alpha() {
        img.to_rgba8().into()
    } else {
        img.to_rgb8().into()
    };
    let mut dst = image::DynamicImage::new(dw, dh, src.color());
    Resizer::new()
        .resize(
            &src,
            &mut dst,
            &ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3)),
        )
        .ok()?;
    Some(dst)
}

/// Store the original; if it decodes as an image, also store a JPEG thumbnail, a
/// downscaled WebP viewer preview (large images), and record its dimensions +
/// whether it has transparency. Runs on a blocking thread.
pub(crate) fn process_and_store(
    storage: &dyn Storage,
    id: &str,
    bytes: Vec<u8>,
    declared_ct: Option<String>,
) -> std::io::Result<Prepared> {
    let image = decode_oriented(&bytes);
    let prepared = match image {
        Some(img) => {
            let content_type = image::guess_format(&bytes)
                .map(|f| f.to_mime_type().to_string())
                .unwrap_or_else(|_| "image/*".to_string());
            let (width, height) = (img.width() as i64, img.height() as i64);
            let has_alpha = img.color().has_alpha();

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

            // Viewer preview: a downscaled WebP for large images so opening one
            // doesn't fetch the full-resolution original. WebP keeps alpha (unlike
            // the JPEG thumbnail), so transparent images get a light preview too.
            // Small images have no preview → the viewer falls back to the original.
            if width > PREVIEW_MAX as i64 || height > PREVIEW_MAX as i64 {
                if let Some(preview) = downscale(&img, PREVIEW_MAX) {
                    let webp = if has_alpha {
                        let rgba = preview.to_rgba8();
                        webp::Encoder::from_rgba(&rgba, rgba.width(), rgba.height())
                            .encode(PREVIEW_QUALITY)
                    } else {
                        let rgb = preview.to_rgb8();
                        webp::Encoder::from_rgb(&rgb, rgb.width(), rgb.height())
                            .encode(PREVIEW_QUALITY)
                    };
                    storage.put(&preview_key(id), &webp)?;
                }
            }

            Prepared {
                content_type,
                width: Some(width),
                height: Some(height),
                has_thumb,
                has_alpha,
            }
        }
        None => Prepared {
            content_type: sanitize_content_type(declared_ct),
            width: None,
            height: None,
            has_thumb: false,
            has_alpha: false,
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

/// `GET /api/attachments/:id/preview` — the downscaled WebP viewer preview if one
/// was generated (large images), else the original bytes. The viewer shows this;
/// Download still points at the full-resolution original.
pub async fn serve_preview(
    State(state): State<AppState>,
    Identity(p): Identity,
    Path(id): Path<String>,
) -> Response {
    let Ok(Some((room_id, att))) = db::lookup_attachment(&state.reads, &id).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !can_access(&state.reads, &p, &room_id).await {
        return StatusCode::FORBIDDEN.into_response();
    }

    // Prefer the preview blob (JPEG); fall back to the original if none exists
    // (small or transparent images, and anything uploaded before previews).
    let storage = state.storage.clone();
    let pkey = preview_key(&id);
    let preview = match tokio::task::spawn_blocking(move || storage.get(&pkey)).await {
        Ok(Ok(b)) => b,
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let (content_type, bytes) = match preview {
        Some(b) => ("image/webp".to_string(), b),
        None => {
            let storage = state.storage.clone();
            let key = id.clone();
            match tokio::task::spawn_blocking(move || storage.get(&key)).await {
                Ok(Ok(Some(b))) => (att.content_type.clone(), b),
                Ok(Ok(None)) => return StatusCode::NOT_FOUND.into_response(),
                _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            }
        }
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
    // are small images, always served whole. Stickers are also served whole (200):
    // a looping <video> sticker always range-requests, and browsers reuse a cached
    // 206 far less reliably than a 200 — so a small sticker would refetch on every
    // remount. Answering the range with the full 200 lets the disk cache serve it.
    if !thumb && !att.is_sticker {
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
                    (
                        header::CONTENT_RANGE,
                        format!("bytes {start}-{end}/{total}"),
                    ),
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

#[cfg(test)]
mod tests {
    use super::{
        header_safe, parse_range, sanitize_content_type, sanitize_filename, size_limit,
        MAX_UPLOAD_BYTES, MAX_VIDEO_BYTES,
    };

    #[test]
    fn range_basic_and_open_ended() {
        assert_eq!(parse_range("bytes=0-99", 1000), Some((0, 99)));
        assert_eq!(parse_range("bytes=100-", 1000), Some((100, 999)));
        assert_eq!(parse_range("bytes=0-", 1000), Some((0, 999)));
        assert_eq!(parse_range(" bytes=10-20 ", 1000), Some((10, 20))); // trimmed
    }

    #[test]
    fn range_suffix() {
        assert_eq!(parse_range("bytes=-100", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=-5000", 1000), Some((0, 999))); // longer than blob
        assert_eq!(parse_range("bytes=-0", 1000), None); // last 0 bytes → invalid
    }

    #[test]
    fn range_clamps_end() {
        assert_eq!(parse_range("bytes=500-5000", 1000), Some((500, 999)));
        assert_eq!(parse_range("bytes=999-999", 1000), Some((999, 999)));
    }

    #[test]
    fn range_rejects_bad() {
        assert_eq!(parse_range("bytes=2000-3000", 1000), None); // start past the end
        assert_eq!(parse_range("bytes=abc", 1000), None);
        assert_eq!(parse_range("bytes=", 1000), None);
        assert_eq!(parse_range("items=0-1", 1000), None); // wrong unit
        assert_eq!(parse_range("bytes=0-99", 0), None); // empty blob
    }

    #[test]
    fn filename_strips_path_and_dangerous_chars() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("a/b\\c.png"), "c.png");
        assert_eq!(sanitize_filename("a\u{0}b\"c.png"), "abc.png");
        assert_eq!(sanitize_filename("   "), "file");
        assert_eq!(sanitize_filename(""), "file");
    }

    #[test]
    fn filename_caps_length() {
        assert_eq!(sanitize_filename(&"x".repeat(500)).len(), 200);
    }

    #[test]
    fn header_safe_strips_quotes_backslashes_controls() {
        // Prevents Content-Disposition header injection.
        assert_eq!(header_safe("a\"b\\c\nd"), "abcd");
        assert_eq!(header_safe("clean.png"), "clean.png");
    }

    #[test]
    fn content_type_validation() {
        assert_eq!(sanitize_content_type(Some("image/png".into())), "image/png");
        assert_eq!(sanitize_content_type(None), "application/octet-stream");
        assert_eq!(
            sanitize_content_type(Some(String::new())),
            "application/octet-stream"
        );
        // Header injection via CRLF is rejected.
        assert_eq!(
            sanitize_content_type(Some("image/png\r\nX: y".into())),
            "application/octet-stream"
        );
        assert_eq!(
            sanitize_content_type(Some("x".repeat(200))),
            "application/octet-stream"
        );
    }

    #[test]
    fn size_limit_video_vs_other() {
        assert_eq!(size_limit(&Some("video/mp4".into())), MAX_VIDEO_BYTES);
        assert_eq!(size_limit(&Some("video/webm".into())), MAX_VIDEO_BYTES);
        assert_eq!(size_limit(&Some("image/png".into())), MAX_UPLOAD_BYTES);
        assert_eq!(size_limit(&None), MAX_UPLOAD_BYTES);
    }

    // --- preview / thumbnail pipeline ---------------------------------------

    use super::process_and_store;
    use crate::storage::{preview_key, thumb_key, Storage};
    use std::collections::HashMap;
    use std::io;
    use std::io::Cursor;
    use std::sync::Mutex;

    /// Minimal in-memory Storage for exercising the upload pipeline.
    #[derive(Default)]
    struct MemStorage(Mutex<HashMap<String, Vec<u8>>>);
    impl Storage for MemStorage {
        fn put(&self, key: &str, bytes: &[u8]) -> io::Result<()> {
            self.0
                .lock()
                .unwrap()
                .insert(key.to_string(), bytes.to_vec());
            Ok(())
        }
        fn get(&self, key: &str) -> io::Result<Option<Vec<u8>>> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        fn remove(&self, key: &str) -> io::Result<()> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
    }

    /// PNG bytes of a solid image of the given size (a real, decodable upload).
    fn png_bytes(w: u32, h: u32, alpha: bool) -> Vec<u8> {
        let mut buf = Vec::new();
        let img = if alpha {
            image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
                w,
                h,
                image::Rgba([10, 120, 200, 128]),
            ))
        } else {
            image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
                w,
                h,
                image::Rgb([10, 120, 200]),
            ))
        };
        img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        buf
    }

    fn is_webp(bytes: &[u8]) -> bool {
        bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP"
    }

    #[test]
    fn large_image_gets_a_webp_preview_and_jpeg_thumb() {
        let store = MemStorage::default();
        let bytes = png_bytes(2000, 1200, false);
        let original_len = bytes.len();
        let prep = process_and_store(&store, "abc", bytes, Some("image/png".into())).unwrap();

        assert!(prep.has_thumb);
        assert!(!prep.has_alpha);
        assert_eq!(prep.width, Some(2000));
        assert_eq!(prep.height, Some(1200));

        // A real WebP preview blob exists and is smaller than the original.
        let preview = store
            .get(&preview_key("abc"))
            .unwrap()
            .expect("preview blob");
        assert!(is_webp(&preview), "preview should be a valid WebP");
        assert!(preview.len() < original_len);
        // Thumbnail is a JPEG (starts with the SOI marker).
        let thumb = store.get(&thumb_key("abc")).unwrap().expect("thumb blob");
        assert_eq!(&thumb[0..2], &[0xFF, 0xD8]);
    }

    #[test]
    fn transparent_image_previews_as_webp_with_alpha_flag() {
        let store = MemStorage::default();
        let prep = process_and_store(
            &store,
            "png",
            png_bytes(1800, 1800, true),
            Some("image/png".into()),
        )
        .unwrap();
        assert!(prep.has_alpha, "alpha channel should be detected");
        let preview = store
            .get(&preview_key("png"))
            .unwrap()
            .expect("preview blob");
        assert!(is_webp(&preview));
    }

    #[test]
    fn small_image_has_no_preview_and_falls_back_to_original() {
        let store = MemStorage::default();
        let prep = process_and_store(
            &store,
            "sm",
            png_bytes(400, 300, false),
            Some("image/png".into()),
        )
        .unwrap();
        assert!(prep.has_thumb);
        // Below PREVIEW_MAX → no preview blob; the endpoint serves the original.
        assert!(store.get(&preview_key("sm")).unwrap().is_none());
        assert!(store.get("sm").unwrap().is_some());
    }
}
