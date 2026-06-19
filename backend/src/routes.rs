//! HTTP routes for auth and admin: link login, session, self-rename, and
//! principal (link) management.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_extra::extract::cookie::CookieJar;
use serde::{Deserialize, Serialize};

use crate::auth::{self, Admin, Identity, Principal, PrincipalSummary, SESSION_COOKIE};
use crate::models::RoomSummary;
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
    match auth::resolve_token(&state.reads, &token).await {
        Ok(Some(p)) => match auth::create_session(&state.db, &p.id).await {
            Ok(session) => {
                let jar = jar.add(auth::session_cookie(session, state.secure_cookies));
                (jar, crate::index_html_response()).into_response()
            }
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        },
        Ok(None) => (StatusCode::NOT_FOUND, "invalid or revoked link").into_response(),
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

// ---- rooms -----------------------------------------------------------------

/// `GET /api/rooms` — rooms the caller may open. Clients get their single room
/// (created on demand); employees get common + every client room.
pub async fn rooms(
    State(state): State<AppState>,
    Identity(p): Identity,
) -> Result<Json<Vec<RoomSummary>>, StatusCode> {
    let list = if p.kind == "user" {
        db::list_rooms_for_user(&state.reads)
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
