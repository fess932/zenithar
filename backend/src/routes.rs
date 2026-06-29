//! HTTP routes for auth and admin: link login, session, self-rename, and
//! principal (link) management.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_extra::extract::cookie::CookieJar;
use serde::{Deserialize, Serialize};

use crate::auth::{
    self, Admin, Identity, IntegrationSummary, Principal, PrincipalSummary, SESSION_COOKIE,
};
use crate::models::{Outbound, RoomSummary, Signal};
use crate::names;
use crate::state::AppState;
use crate::{db, now_millis};

/// Reject cross-origin mutations. If there's no Origin (non-browser client) we
/// allow it; SameSite=Lax already covers the common browser CSRF case.
pub(crate) fn origin_ok(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) else {
        return true;
    };
    match headers.get("host").and_then(|v| v.to_str().ok()) {
        Some(host) => origin.ends_with(host),
        None => false,
    }
}

// ---- login by link ---------------------------------------------------------

/// Best-effort client IP for rate limiting: trust the reverse proxy's
/// `X-Forwarded-For` (first hop) / `X-Real-IP` when present, else a constant key
/// (so a direct, proxy-less deploy still gets a single shared bucket).
pub(crate) fn client_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(str::trim)
        })
        .unwrap_or("direct")
        .to_string()
}

/// `GET /i/:token` — resolve a link-token, mint a session cookie, serve the SPA.
pub async fn enter_link(
    Path(token): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Response {
    // Throttle link attempts per IP (tokens are 256-bit, so this is defense in
    // depth against enumeration / hammering, not the primary protection).
    if !state.limits.login.check(&client_ip(&headers)) {
        return (StatusCode::TOO_MANY_REQUESTS, "slow down").into_response();
    }
    // Debug aid for the desktop/mobile deep-link login: shows the request reached
    // the server, the outcome, and the User-Agent (so app vs browser is clear).
    let ua = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("?");
    match auth::resolve_token(&state.reads, &token).await {
        Ok(Some(p)) => match auth::create_session(&state.db, &p.id).await {
            Ok(session) => {
                tracing::info!(principal = %p.id, %ua, "link login OK");
                let jar = jar.add(auth::session_cookie(session, state.secure_cookies));
                (jar, crate::index_html_response()).into_response()
            }
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        },
        Ok(None) => {
            tracing::info!(%ua, "link login: invalid or revoked token");
            (StatusCode::NOT_FOUND, "invalid or revoked link").into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

// ---- session / self ---------------------------------------------------------

pub async fn me(State(state): State<AppState>, jar: CookieJar) -> Json<Option<Principal>> {
    Json(auth::principal_from_jar(&state, &jar).await)
}

pub async fn logout(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    if let Some(c) = jar.get(SESSION_COOKIE) {
        let _ = auth::delete_session(&state.db, c.value()).await;
    }
    (
        jar.add(auth::clear_cookie(state.secure_cookies)),
        StatusCode::OK,
    )
}

#[derive(Deserialize)]
pub struct RenameReq {
    pub display_name: String,
}

pub async fn rename(
    State(state): State<AppState>,
    Identity(p): Identity,
    headers: HeaderMap,
    Json(body): Json<RenameReq>,
) -> Response {
    if !origin_ok(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if p.kind != "user" {
        return StatusCode::FORBIDDEN.into_response(); // clients are anonymous
    }
    let name = body.display_name.trim();
    if name.is_empty() || name.chars().count() > 40 {
        return (StatusCode::BAD_REQUEST, "name must be 1–40 chars").into_response();
    }
    match auth::set_display_name(&state.db, &p.id, name).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `POST /api/me/app-link` — mint a fresh link-token for the signed-in principal
/// and hand back a `zenithar://i/<token>` deep link. Opening it launches the
/// desktop app and logs it in as the same principal (the app just navigates to
/// the existing `/i/<token>` login). The token is ADDITIVE — it doesn't revoke
/// the user's existing link.
pub async fn app_link(
    State(state): State<AppState>,
    Identity(p): Identity,
    headers: HeaderMap,
) -> Response {
    if !origin_ok(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    // Return the web login PATH; the frontend wraps it with its own origin into
    // the `zenithar://login?u=…` deep link, so the app logs into THIS host.
    match auth::issue_token(&state.db, &p.id, None).await {
        Ok(token) => Json(serde_json::json!({ "web": format!("/i/{token}") })).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize)]
pub struct PushTokenReq {
    pub token: String,
    #[serde(default = "default_platform")]
    pub platform: String,
}

fn default_platform() -> String {
    "android".to_string()
}

/// `POST /api/push/register` — store this device's FCM token for the signed-in
/// principal, so messages that arrive while they're offline reach the device as
/// a push notification. Idempotent (keyed on the token).
pub async fn push_register(
    State(state): State<AppState>,
    Identity(p): Identity,
    headers: HeaderMap,
    Json(body): Json<PushTokenReq>,
) -> Response {
    if !origin_ok(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let token = body.token.trim();
    if token.is_empty() || token.len() > 4096 {
        return (StatusCode::BAD_REQUEST, "invalid token").into_response();
    }
    match db::upsert_push_token(&state.db, token, &p.id, &body.platform, now_millis()).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `POST /api/push/unregister` — forget a device token (logout / opt-out).
pub async fn push_unregister(
    State(state): State<AppState>,
    Identity(_p): Identity,
    headers: HeaderMap,
    Json(body): Json<PushTokenReq>,
) -> Response {
    if !origin_ok(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match db::delete_push_token(&state.db, body.token.trim()).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize)]
pub struct DmReq {
    pub with: String,
}

/// `POST /api/dm` — open (or create) the 1:1 direct room with another employee.
/// Idempotent: returns the same room id from either side. Employees only.
pub async fn start_dm(
    State(state): State<AppState>,
    Identity(p): Identity,
    headers: HeaderMap,
    Json(body): Json<DmReq>,
) -> Response {
    if !origin_ok(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if p.kind != "user" {
        return StatusCode::FORBIDDEN.into_response(); // DMs are employee↔employee
    }
    if body.with == p.id {
        return (StatusCode::BAD_REQUEST, "cannot message yourself").into_response();
    }
    match db::principal_kind(&state.reads, &body.with).await {
        Ok(Some(k)) if k == "user" => {}
        _ => return (StatusCode::BAD_REQUEST, "no such teammate").into_response(),
    }
    match db::ensure_direct_room(&state.db, &p.id, &body.with).await {
        Ok(room_id) => {
            // Nudge the other side so the new DM shows in their room list.
            let _ = state.signal.send(Signal {
                room_id: room_id.clone(),
                target: Some(body.with.clone()),
                exclude: None,
                all_employees: false,
                frame: Outbound::RoomsChanged,
            });
            Json(serde_json::json!({ "room_id": room_id })).into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

// ---- ice servers (WebRTC) --------------------------------------------------

/// `GET /api/ice` — the ICE servers the browser should use (STUN/TURN), straight
/// from `ZENITHAR_ICE_SERVERS` (a JSON array of `RTCIceServer`). Served to the
/// client so STUN/TURN can change without rebuilding the frontend. Public: it's
/// just config, no secrets beyond whatever TURN creds the operator puts there.
pub async fn ice_servers() -> Json<serde_json::Value> {
    let raw = std::env::var("ZENITHAR_ICE_SERVERS").unwrap_or_default();
    Json(serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!([])))
}

// ---- rooms -----------------------------------------------------------------

/// `GET /api/rooms` — rooms the caller may open. Clients get their single room
/// (created on demand); employees get common + every client room.
pub async fn rooms(
    State(state): State<AppState>,
    Identity(p): Identity,
) -> Result<Json<Vec<RoomSummary>>, StatusCode> {
    let list = if p.kind == "user" {
        db::list_rooms_for_user(&state.reads, &p.id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        let id = db::ensure_client_room(&state.db, &p.id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        vec![RoomSummary {
            id,
            kind: "client".to_string(),
            title: None,
            client_id: Some(p.id.clone()),
            created_at: now_millis(),
        }]
    };
    Ok(Json(list))
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    #[serde(default)]
    pub before: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

/// `GET /api/rooms/:id/messages?before&limit` — a page of older messages for the
/// browser (cookie auth), oldest-first. Lets the chat lazily load history when
/// scrolled up; `before` is the oldest message id the client already has.
pub async fn room_messages(
    State(state): State<AppState>,
    Identity(p): Identity,
    Path(room_id): Path<String>,
    Query(q): Query<HistoryQuery>,
) -> Response {
    // Membership-aware: employees see common/client rooms + their own DMs, a
    // client only its own room. (Was `room_exists`, which leaked DMs.)
    let allowed = db::can_access_room(&state.reads, &p.kind, &p.id, &room_id)
        .await
        .unwrap_or(false);
    if !allowed {
        return StatusCode::FORBIDDEN.into_response();
    }
    let limit = q.limit.unwrap_or(50).clamp(1, 100);
    match db::messages_before(&state.reads, &room_id, limit, q.before.as_deref()).await {
        Ok(msgs) => Json(msgs).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

// ---- admin: principals (links) ---------------------------------------------

#[derive(Deserialize)]
pub struct CreatePrincipal {
    pub kind: String, // "user" | "client"
    #[serde(default)]
    pub display: Option<String>,
}

#[derive(Serialize)]
pub struct LinkResp {
    pub principal_id: String,
    pub url: String,
}

pub async fn create_principal(
    State(state): State<AppState>,
    _admin: Admin,
    headers: HeaderMap,
    Json(body): Json<CreatePrincipal>,
) -> Response {
    if !origin_ok(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let kind = match body.kind.as_str() {
        "user" | "client" => body.kind.as_str(),
        _ => return (StatusCode::BAD_REQUEST, "kind must be user|client").into_response(),
    };
    let name = body
        .display
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(names::random_name);

    match issue_for_new(&state, kind, &name).await {
        Ok(resp) => Json(resp).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn issue_for_new(state: &AppState, kind: &str, name: &str) -> sqlx::Result<LinkResp> {
    let p = auth::create_principal(&state.db, kind, name, false).await?;
    // A client gets a dedicated room up front so employees see it immediately.
    if kind == "client" {
        db::ensure_client_room(&state.db, &p.id).await?;
    }
    let token = auth::issue_token(&state.db, &p.id, None).await?;
    Ok(LinkResp {
        principal_id: p.id,
        url: format!("/i/{token}"),
    })
}

pub async fn rotate(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !origin_ok(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match auth::rotate_token(&state.db, &id).await {
        Ok(token) => Json(LinkResp {
            principal_id: id,
            url: format!("/i/{token}"),
        })
        .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn revoke(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !origin_ok(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match auth::revoke_tokens(&state.db, &id).await {
        Ok(_) => StatusCode::OK.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn list_principals(
    State(state): State<AppState>,
    _admin: Admin,
) -> Result<Json<Vec<PrincipalSummary>>, StatusCode> {
    auth::list_principals(&state.reads)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

// ---- admin: integrations (API tokens) --------------------------------------

#[derive(Deserialize)]
pub struct CreateIntegration {
    pub name: String,
}

#[derive(Serialize)]
pub struct IntegrationToken {
    pub id: String,
    pub name: String,
    /// Plaintext API token — shown once, store it now.
    pub token: String,
}

pub async fn list_integrations(
    State(state): State<AppState>,
    _admin: Admin,
) -> Result<Json<Vec<IntegrationSummary>>, StatusCode> {
    auth::list_integrations(&state.reads)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub async fn create_integration(
    State(state): State<AppState>,
    _admin: Admin,
    headers: HeaderMap,
    Json(body): Json<CreateIntegration>,
) -> Response {
    if !origin_ok(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let name = body.name.trim();
    if name.is_empty() || name.chars().count() > 40 {
        return (StatusCode::BAD_REQUEST, "name must be 1–40 chars").into_response();
    }
    match auth::create_integration(&state.db, name).await {
        Ok((id, token)) => Json(IntegrationToken {
            id,
            name: name.to_string(),
            token,
        })
        .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn rotate_integration(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !origin_ok(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match auth::rotate_api_token(&state.db, &id).await {
        Ok(token) => Json(IntegrationToken {
            id,
            name: String::new(),
            token,
        })
        .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn revoke_integration(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !origin_ok(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match auth::revoke_api_tokens(&state.db, &id).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

// ---- connections (presence list) -------------------------------------------

#[derive(Serialize)]
pub struct Person {
    id: String,
    name: String,
    kind: String,
    online: bool,
    /// Unix millis of last activity (last received frame); `None` if never seen
    /// since the server started.
    last_seen: Option<i64>,
    /// Last WS ping round-trip in ms (only while online).
    ping_ms: Option<i64>,
}

/// `GET /api/people` — the connections list: every human principal with live
/// online status + last-seen. Employees only (clients don't get a team roster).
pub async fn people(State(state): State<AppState>, Identity(p): Identity) -> Response {
    if !auth::is_staff(&p.kind) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let roster = match db::list_people(&state.reads).await {
        Ok(r) => r,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let last_seen = state.presence.last_seen_map();
    let pings = state.presence.ping_map();
    let people: Vec<Person> = roster
        .into_iter()
        .map(|(id, name, kind)| {
            let online = state.presence.is_online(&id);
            let last = last_seen.get(&id).copied();
            let ping_ms = if online {
                pings.get(&id).copied()
            } else {
                None
            };
            Person {
                online,
                last_seen: last,
                ping_ms,
                id,
                name,
                kind,
            }
        })
        .collect();
    Json(people).into_response()
}

// ---- admin: telemetry dashboard --------------------------------------------

#[derive(Serialize)]
pub struct TelemetryInfo {
    /// Whether trace export is configured (ZENITHAR_OTLP_ENDPOINT set).
    enabled: bool,
    /// GreptimeDB HTTP port (parsed from the OTLP endpoint; its `/dashboard`
    /// lives here). The client builds the URL with its own host, since the
    /// configured endpoint host may be server-local (127.0.0.1).
    port: u16,
}

/// `GET /api/admin/telemetry` — tells the admin UI whether to show a link to the
/// GreptimeDB dashboard, and on which port.
pub async fn telemetry_info(_admin: Admin) -> Json<TelemetryInfo> {
    let endpoint = std::env::var("ZENITHAR_OTLP_ENDPOINT")
        .ok()
        .filter(|s| !s.is_empty());
    let port = endpoint.as_deref().and_then(otlp_port).unwrap_or(4000);
    Json(TelemetryInfo {
        enabled: endpoint.is_some(),
        port,
    })
}

/// Pull the port out of an `http://host:port/...` OTLP endpoint.
fn otlp_port(url: &str) -> Option<u16> {
    url.split("://")
        .nth(1)?
        .split('/')
        .next()?
        .rsplit(':')
        .next()?
        .parse()
        .ok()
}
