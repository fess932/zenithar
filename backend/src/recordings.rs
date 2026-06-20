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
    let names = db::all_principal_names(&state.reads).await.unwrap_or_default();
    let by_call = scan_tracks(&state.recordings_dir);

    let views: Vec<RecordingView> = calls
        .into_iter()
        .map(|c| {
            let tracks = by_call
                .get(&c.call_id)
                .map(|pids| {
                    pids.iter()
                        .map(|pid| Track {
                            participant_name: names.get(pid).cloned().unwrap_or_else(|| pid.clone()),
                            url: format!("/api/admin/recordings/{}/{}", c.call_id, pid),
                            participant_id: pid.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default();
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
    let path = state
        .recordings_dir
        .join(format!("{call_id}.{participant_id}.ogg"));
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

/// Group the recordings dir into `call_id → [participant_id]` from the filenames.
fn scan_tracks(dir: &Path) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return map;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        // "<call_id>.<participant_id>.ogg" — both ids are dot-free ULIDs.
        if let Some(stem) = name.strip_suffix(".ogg") {
            if let Some((call_id, participant_id)) = stem.split_once('.') {
                map.entry(call_id.to_string())
                    .or_default()
                    .push(participant_id.to_string());
            }
        }
    }
    map
}

fn is_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric())
}
