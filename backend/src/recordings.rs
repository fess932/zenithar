//! Admin recordings page: list + stream the server-side call recordings.
//!
//! Recordings are written by [`crate::calls`] as one Ogg/Opus file per
//! participant, named `<call_id>.<participant_id>.ogg` under the recordings dir.
//! Here we join that on-disk layout with the `calls` table (for room + caller +
//! timestamps) and serve each track to an admin `<audio>` player. Admin-only.

use std::collections::HashMap;
use std::path::Path;

use axum::extract::{Path as AxPath, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use serde::Serialize;

use crate::auth::Admin;
use crate::db;
use crate::state::AppState;

/// One participant's audio track within a recorded call.
#[derive(Serialize)]
pub struct Track {
    participant_id: String,
    participant_name: String,
    /// Where the admin player fetches the Ogg/Opus bytes.
    url: String,
}

/// A recorded call with its per-participant tracks.
#[derive(Serialize)]
pub struct RecordingView {
    call_id: String,
    room_title: Option<String>,
    started_by_name: Option<String>,
    started_at: i64,
    ended_at: Option<i64>,
    tracks: Vec<Track>,
}

/// `GET /api/admin/recordings` — recorded calls (newest first) with their tracks.
pub async fn list(State(state): State<AppState>, _admin: Admin) -> Response {
    let Ok(calls) = db::list_recorded_calls(&state.reads).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let names = db::all_principal_names(&state.reads)
        .await
        .unwrap_or_default();
    let by_call = scan_tracks(&state.recordings_dir);

    let views: Vec<RecordingView> = calls
        .into_iter()
        .map(|c| {
            // Prefer the single mixed file; fall back to per-track for old calls.
            let tracks = match by_call.get(&c.call_id) {
                Some(cf) if cf.mix => vec![Track {
                    participant_id: "mix".to_string(),
                    participant_name: String::new(), // one whole-call recording
                    url: format!("/api/admin/recordings/{}/mix", c.call_id),
                }],
                Some(cf) => cf
                    .parts
                    .iter()
                    .map(|pid| Track {
                        participant_name: names.get(pid).cloned().unwrap_or_else(|| pid.clone()),
                        url: format!("/api/admin/recordings/{}/{}", c.call_id, pid),
                        participant_id: pid.clone(),
                    })
                    .collect(),
                None => Vec::new(),
            };
            RecordingView {
                call_id: c.call_id,
                room_title: c.room_title,
                started_by_name: c.started_by_name,
                started_at: c.started_at,
                ended_at: c.ended_at,
                tracks,
            }
        })
        .collect();

    Json(views).into_response()
}

/// `GET /api/admin/recordings/{call_id}/{participant_id}` — the Ogg/Opus bytes.
pub async fn serve(
    State(state): State<AppState>,
    _admin: Admin,
    AxPath((call_id, participant_id)): AxPath<(String, String)>,
) -> Response {
    // ids are ULIDs (alnum, no dots/slashes) — reject anything else so the path
    // can't escape the recordings dir.
    if !is_id(&call_id) || !is_id(&participant_id) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    // "mix" → the single mixed file; otherwise a per-participant track. Both Opus.
    let file = if participant_id == "mix" {
        format!("{call_id}.mix.ogg")
    } else {
        format!("{call_id}.{participant_id}.ogg")
    };
    let path = state.recordings_dir.join(file);
    let bytes = match tokio::task::spawn_blocking(move || std::fs::read(path)).await {
        Ok(Ok(b)) => b,
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    (
        [
            (header::CONTENT_TYPE, "audio/ogg".to_string()),
            (header::CACHE_CONTROL, "private, max-age=3600".to_string()),
        ],
        bytes,
    )
        .into_response()
}

/// `DELETE /api/admin/recordings/{call_id}` — remove all audio files for the call
/// (mixed + per-track) from disk and clear the DB flag so it leaves the list.
pub async fn delete(
    State(state): State<AppState>,
    _admin: Admin,
    AxPath(call_id): AxPath<String>,
) -> Response {
    if !is_id(&call_id) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let dir = state.recordings_dir.clone();
    let cid = call_id.clone();
    let _ = tokio::task::spawn_blocking(move || remove_call_files(&dir, &cid)).await;
    // Best-effort DB flag clear; the files are already gone either way.
    let _ = db::clear_recording(&state.db, &call_id).await;
    StatusCode::NO_CONTENT.into_response()
}

/// Delete every `<call_id>.*.ogg` file in the recordings dir. `call_id` is a
/// dot-free ULID, so the `<call_id>.` prefix matches only this call's files.
fn remove_call_files(dir: &Path, call_id: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let prefix = format!("{call_id}.");
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with(&prefix) && name.ends_with(".ogg") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// What's on disk for a call: the mixed file and/or the per-participant tracks.
#[derive(Default)]
struct CallFiles {
    mix: bool,
    parts: Vec<String>,
}

/// Group the recordings dir by `call_id`: `<call>.mix.ogg` (mixed) and
/// `<call>.<participant>.ogg` (per-track) from the filenames.
fn scan_tracks(dir: &Path) -> HashMap<String, CallFiles> {
    let mut map: HashMap<String, CallFiles> = HashMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return map;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if let Some(call_id) = name.strip_suffix(".mix.ogg") {
            map.entry(call_id.to_string()).or_default().mix = true;
        } else if let Some(stem) = name.strip_suffix(".ogg") {
            // "<call_id>.<participant_id>.ogg" — both ids are dot-free ULIDs.
            if let Some((call_id, participant_id)) = stem.split_once('.') {
                map.entry(call_id.to_string())
                    .or_default()
                    .parts
                    .push(participant_id.to_string());
            }
        }
    }
    map
}

fn is_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric())
}
