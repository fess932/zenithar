//! REST API for integrations (Phase 6), under `/api/v1`, authenticated by an
//! `Authorization: Bearer zk_…` token (a `bot` principal). Mirrors what the chat
//! UI can do: read rooms/history, post messages (with attachments), create new
//! client links, and address a client's room by its id.
//!
//! Bots are "staff" (full room access). Posting reuses the same delivery path as
//! the WebSocket (`crate::send::deliver`), so API messages show up live for
//! connected clients and persist identically.

use axum::extract::{Multipart, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::auth::{self, ApiAuth, Principal};
use crate::models::{ChatMessage, RoomSummary};
use crate::state::AppState;
use crate::{db, names, now_millis, uploads};

const MAX_ATTACHMENTS: usize = 5;
const DEFAULT_PAGE: i64 = 50;
const MAX_PAGE: i64 = 100;

/// `GET /api/v1/me` — identify the calling integration (handy for testing creds).
#[derive(Serialize)]
pub struct Whoami {
    pub id: String,
    pub name: String,
    pub kind: String,
}

pub async fn me(ApiAuth(p): ApiAuth) -> Json<Whoami> {
    Json(Whoami {
        id: p.id,
        name: p.display_name,
        kind: p.kind,
    })
}

/// `GET /api/v1/rooms` — every room (common + each client room). `client_id`
/// lets the integration map a client to its room.
pub async fn rooms(
    State(state): State<AppState>,
    ApiAuth(bot): ApiAuth,
) -> Result<Json<Vec<RoomSummary>>, StatusCode> {
    // A bot has no DMs, so this returns just common + every client room.
    db::list_rooms_for_user(&state.reads, &bot.id)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Deserialize)]
pub struct PageQuery {
    pub limit: Option<i64>,
    /// Return messages strictly older than this message id (backward pagination).
    pub before: Option<String>,
}

/// `GET /api/v1/rooms/{id}/messages?limit&before` — a page of history, oldest-first.
pub async fn get_messages(
    State(state): State<AppState>,
    ApiAuth(bot): ApiAuth,
    Path(room_id): Path<String>,
    Query(q): Query<PageQuery>,
) -> Response {
    // A bot reaches common/client rooms but never DMs (it's not a member).
    if !db::staff_can_open(&state.reads, &bot.id, &room_id)
        .await
        .unwrap_or(false)
    {
        return (StatusCode::NOT_FOUND, "room not found").into_response();
    }
    let limit = q.limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE);
    match db::messages_before(&state.reads, &room_id, limit, q.before.as_deref()).await {
        Ok(msgs) => Json(msgs).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize)]
pub struct SendBody {
    pub body: String,
    #[serde(default)]
    pub reply_to: Option<String>,
    #[serde(default)]
    pub attachment_ids: Vec<String>,
}

/// `POST /api/v1/rooms/{id}/messages` — post to a room as the bot.
pub async fn post_message(
    State(state): State<AppState>,
    ApiAuth(bot): ApiAuth,
    Path(room_id): Path<String>,
    Json(body): Json<SendBody>,
) -> Response {
    send_as(&state, &bot, &room_id, body, false).await
}

/// `POST /api/v1/clients/{client_id}/messages` — post to a client's room by the
/// client's principal id (no need to know the room id).
pub async fn post_client_message(
    State(state): State<AppState>,
    ApiAuth(bot): ApiAuth,
    Path(client_id): Path<String>,
    Json(body): Json<SendBody>,
) -> Response {
    match db::room_of_client(&state.reads, &client_id).await {
        Ok(Some(room_id)) => send_as(&state, &bot, &room_id, body, false).await,
        Ok(None) => (StatusCode::NOT_FOUND, "client not found").into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize)]
pub struct CreateClient {
    /// Display name for the new client (random if omitted).
    #[serde(default)]
    pub name: Option<String>,
    /// Optional first message — e.g. a short order description. Posted as the
    /// client, so it reads like their incoming request and pings employees.
    #[serde(default)]
    pub order: Option<String>,
}

#[derive(Serialize)]
pub struct CreatedClient {
    pub client_id: String,
    pub room_id: String,
    /// Relative login link (`/i/<token>`) — prepend your host to share it.
    pub url: String,
}

/// `POST /api/v1/clients` — create a new client + their room + login link, and
/// optionally seed the room with a first message (the order description).
pub async fn create_client(
    State(state): State<AppState>,
    _auth: ApiAuth,
    Json(body): Json<CreateClient>,
) -> Response {
    let name = body
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(names::random_name);

    let client = match auth::create_principal(&state.db, "client", &name, false).await {
        Ok(p) => p,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let room_id = match db::ensure_client_room(&state.db, &client.id).await {
        Ok(id) => id,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let token = match auth::issue_token(&state.db, &client.id, None).await {
        Ok(t) => t,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    // Seed the order description as the client's own first message (notifies
    // employees, just like a real anonymous client writing in).
    if let Some(order) = body
        .order
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let seed = SendBody {
            body: order.to_string(),
            reply_to: None,
            attachment_ids: Vec::new(),
        };
        let _ = build_and_deliver(&state, &client, &room_id, seed, true).await;
    }

    Json(CreatedClient {
        client_id: client.id,
        room_id,
        url: format!("/i/{token}"),
    })
    .into_response()
}

/// `POST /api/v1/uploads` — multipart `room_id` + `file`, as the bot. Returns the
/// attachment meta; pass its `id` in a later `attachment_ids` on a message.
pub async fn upload(
    State(state): State<AppState>,
    ApiAuth(bot): ApiAuth,
    multipart: Multipart,
) -> Response {
    uploads::ingest(&state, &bot, multipart).await
}

// ---- shared send -----------------------------------------------------------

/// Validate + deliver a message as `author` to `room_id`, returning the created
/// message as JSON (or an error response).
async fn send_as(
    state: &AppState,
    author: &Principal,
    room_id: &str,
    body: SendBody,
    notify_employees: bool,
) -> Response {
    match build_and_deliver(state, author, room_id, body, notify_employees).await {
        Ok(chat) => Json(chat).into_response(),
        Err((code, msg)) => (code, msg).into_response(),
    }
}

async fn build_and_deliver(
    state: &AppState,
    author: &Principal,
    room_id: &str,
    body: SendBody,
    notify_employees: bool,
) -> Result<ChatMessage, (StatusCode, &'static str)> {
    if !db::staff_can_open(&state.reads, &author.id, room_id)
        .await
        .unwrap_or(false)
    {
        return Err((StatusCode::NOT_FOUND, "room not found"));
    }

    // Resolve up to 5 attachments; each must belong to this room.
    let mut attachments = Vec::new();
    for aid in body.attachment_ids.into_iter().take(MAX_ATTACHMENTS) {
        match db::lookup_attachment(&state.reads, &aid).await {
            Ok(Some((room, att))) if room == room_id => attachments.push(att),
            _ => return Err((StatusCode::BAD_REQUEST, "bad attachment")),
        }
    }

    if body.body.trim().is_empty() && attachments.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "empty message"));
    }

    let reply_to = match body.reply_to {
        Some(rid) => db::reply_preview(&state.reads, &rid, room_id)
            .await
            .unwrap_or(None),
        None => None,
    };

    let chat = ChatMessage {
        id: Ulid::new().to_string(),
        room_id: room_id.to_string(),
        author_id: author.id.clone(),
        author_name: author.display_name.clone(),
        body: body.body,
        reply_to,
        client_msg_id: None,
        created_at: now_millis(),
        edited_at: None,
        attachments,
    };

    crate::send::deliver(state, chat.clone(), notify_employees)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "writer unavailable"))?;
    Ok(chat)
}
