//! HTTP routes for auth and admin: link login, session, self-rename, and
//! principal (link) management.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_extra::extract::cookie::CookieJar;
use serde::{Deserialize, Serialize};

use crate::auth::{self, Admin, Identity, Principal, PrincipalSummary, SESSION_COOKIE};
use crate::names;
use crate::state::AppState;

/// Reject cross-origin mutations. If there's no Origin (non-browser client) we
/// allow it; SameSite=Lax already covers the common browser CSRF case.
fn origin_ok(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) else {
        return true;
    };
    match headers.get("host").and_then(|v| v.to_str().ok()) {
        Some(host) => origin.ends_with(host),
        None => false,
    }
}

// ---- login by link ---------------------------------------------------------

/// `GET /i/:token` — resolve a link-token, mint a session cookie, serve the SPA.
pub async fn enter_link(
    Path(token): Path<String>,
    State(state): State<AppState>,
    jar: CookieJar,
) -> Response {
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
