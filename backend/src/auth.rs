//! Passwordless auth: link-tokens exchanged for cookie sessions, plus the
//! `principals` model (employees + anonymous clients). Tokens are stored only as
//! a SHA-256 hash; the plaintext is shown once at issue time.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use ulid::Ulid;

use crate::now_millis;
use crate::state::AppState;

pub const SESSION_COOKIE: &str = "zsid";
const SESSION_TTL_MS: i64 = 30 * 24 * 60 * 60 * 1000; // 30 days

/// An authenticated identity (employee or client), from the session cookie.
pub struct Identity(pub Principal);

/// An authenticated admin (employee with is_admin). The principal is kept for
/// future auditing of admin actions.
pub struct Admin(#[allow(dead_code)] pub Principal);

/// An authenticated integration, from an `Authorization: Bearer zk_…` header.
/// The wrapped principal is a `bot` (full room access, never a browser session).
pub struct ApiAuth(pub Principal);

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct Principal {
    pub id: String,
    pub kind: String, // "user" | "client"
    pub display_name: String,
    pub is_admin: bool,
    /// Emoji grapheme, `"photo:<millis>"`, or None (client renders a default).
    pub avatar: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PrincipalSummary {
    pub id: String,
    pub kind: String,
    pub display_name: String,
    pub is_admin: bool,
    pub created_at: i64,
    pub active: bool, // has a non-revoked token (a live link)
}

// ---- crypto helpers --------------------------------------------------------

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    getrandom::getrandom(&mut buf).expect("OS RNG unavailable");
    buf
}

/// A fresh URL-safe random token (~256 bits).
fn new_token() -> String {
    URL_SAFE_NO_PAD.encode(random_bytes::<32>())
}

/// Store/lookup key for a token — never store plaintext.
fn hash_token(token: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(token.as_bytes()))
}

// ---- principals ------------------------------------------------------------

pub async fn create_principal(
    db: &SqlitePool,
    kind: &str,
    display_name: &str,
    is_admin: bool,
) -> sqlx::Result<Principal> {
    let id = Ulid::new().to_string();
    sqlx::query(
        "INSERT INTO principals (id, kind, display_name, is_admin, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(&id)
    .bind(kind)
    .bind(display_name)
    .bind(is_admin)
    .bind(now_millis())
    .execute(db)
    .await?;
    Ok(Principal {
        id,
        kind: kind.to_string(),
        display_name: display_name.to_string(),
        is_admin,
        avatar: None, // freshly created; the client draws a default until set
    })
}

pub async fn count_principals(reads: &SqlitePool) -> sqlx::Result<i64> {
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM principals")
        .fetch_one(reads)
        .await?;
    Ok(n)
}

pub async fn list_principals(reads: &SqlitePool) -> sqlx::Result<Vec<PrincipalSummary>> {
    // Bots (API integrations) are managed in their own admin section, not here.
    sqlx::query_as::<_, PrincipalSummary>(
        "SELECT p.id, p.kind, p.display_name, p.is_admin, p.created_at,
                EXISTS(SELECT 1 FROM tokens t
                       WHERE t.principal_id = p.id AND t.revoked_at IS NULL) AS active
         FROM principals p
         WHERE p.kind IN ('user', 'client')
         ORDER BY p.created_at DESC",
    )
    .fetch_all(reads)
    .await
}

/// Staff = principals with full room access (employees + integration bots),
/// as opposed to a `client` who only ever sees its own room.
pub fn is_staff(kind: &str) -> bool {
    kind == "user" || kind == "bot"
}

pub async fn set_display_name(db: &SqlitePool, principal_id: &str, name: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE principals SET display_name = ?1 WHERE id = ?2")
        .bind(name)
        .bind(principal_id)
        .execute(db)
        .await?;
    Ok(())
}

/// Set (or clear, with None) a principal's avatar. The value is an emoji grapheme
/// or `"photo:<millis>"`; clearing restores the client-side default.
pub async fn set_avatar(
    db: &SqlitePool,
    principal_id: &str,
    avatar: Option<&str>,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE principals SET avatar = ?1 WHERE id = ?2")
        .bind(avatar)
        .bind(principal_id)
        .execute(db)
        .await?;
    Ok(())
}

/// The current avatar value for a principal (None if unset / unknown).
pub async fn get_avatar(reads: &SqlitePool, principal_id: &str) -> sqlx::Result<Option<String>> {
    sqlx::query_scalar("SELECT avatar FROM principals WHERE id = ?1")
        .bind(principal_id)
        .fetch_optional(reads)
        .await
        .map(Option::flatten)
}

// ---- link tokens -----------------------------------------------------------

/// Issue a fresh link-token for a principal; returns the plaintext (shown once).
pub async fn issue_token(
    db: &SqlitePool,
    principal_id: &str,
    rotated_from: Option<&str>,
) -> sqlx::Result<String> {
    let token = new_token();
    sqlx::query(
        "INSERT INTO tokens (id, token_hash, principal_id, created_at, rotated_from)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(Ulid::new().to_string())
    .bind(hash_token(&token))
    .bind(principal_id)
    .bind(now_millis())
    .bind(rotated_from)
    .execute(db)
    .await?;
    Ok(token)
}

/// Revoke all active tokens of a principal. Returns the id of the most recent
/// one revoked (for `rotated_from` chaining), if any.
pub async fn revoke_tokens(db: &SqlitePool, principal_id: &str) -> sqlx::Result<Option<String>> {
    let prev: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM tokens
         WHERE principal_id = ?1 AND revoked_at IS NULL
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(principal_id)
    .fetch_optional(db)
    .await?;

    sqlx::query("UPDATE tokens SET revoked_at = ?1 WHERE principal_id = ?2 AND revoked_at IS NULL")
        .bind(now_millis())
        .bind(principal_id)
        .execute(db)
        .await?;

    Ok(prev.map(|(id,)| id))
}

/// Revoke the principal's current token(s) and issue a new one. Returns plaintext.
pub async fn rotate_token(db: &SqlitePool, principal_id: &str) -> sqlx::Result<String> {
    let prev = revoke_tokens(db, principal_id).await?;
    issue_token(db, principal_id, prev.as_deref()).await
}

/// Resolve a link-token to its principal (if the token exists and isn't revoked).
pub async fn resolve_token(reads: &SqlitePool, token: &str) -> sqlx::Result<Option<Principal>> {
    sqlx::query_as::<_, Principal>(
        "SELECT p.id, p.kind, p.display_name, p.is_admin, p.avatar
         FROM tokens t JOIN principals p ON p.id = t.principal_id
         WHERE t.token_hash = ?1 AND t.revoked_at IS NULL",
    )
    .bind(hash_token(token))
    .fetch_optional(reads)
    .await
}

// ---- api tokens (integrations) ---------------------------------------------

/// API tokens are prefixed so they're recognizable in logs / config and can be
/// told apart from link/session tokens at a glance.
const API_TOKEN_PREFIX: &str = "zk_";

/// A fresh API token: `zk_` + ~256 bits of URL-safe randomness.
fn new_api_token() -> String {
    format!(
        "{API_TOKEN_PREFIX}{}",
        URL_SAFE_NO_PAD.encode(random_bytes::<32>())
    )
}

/// Admin view of one integration (a `bot` principal + its current API token).
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct IntegrationSummary {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
    pub active: bool, // has a non-revoked API token
}

/// Create a new integration: a `bot` principal plus its first API token.
/// Returns `(principal_id, plaintext_token)` — the token is shown once.
pub async fn create_integration(db: &SqlitePool, name: &str) -> sqlx::Result<(String, String)> {
    let p = create_principal(db, "bot", name, false).await?;
    let token = issue_api_token(db, &p.id, name).await?;
    Ok((p.id, token))
}

/// Issue a fresh API token for a (bot) principal; returns the plaintext.
pub async fn issue_api_token(
    db: &SqlitePool,
    principal_id: &str,
    name: &str,
) -> sqlx::Result<String> {
    let token = new_api_token();
    sqlx::query(
        "INSERT INTO api_tokens (id, token_hash, principal_id, name, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(Ulid::new().to_string())
    .bind(hash_token(&token))
    .bind(principal_id)
    .bind(name)
    .bind(now_millis())
    .execute(db)
    .await?;
    Ok(token)
}

/// Revoke all active API tokens of an integration.
pub async fn revoke_api_tokens(db: &SqlitePool, principal_id: &str) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE api_tokens SET revoked_at = ?1
         WHERE principal_id = ?2 AND revoked_at IS NULL",
    )
    .bind(now_millis())
    .bind(principal_id)
    .execute(db)
    .await?;
    Ok(())
}

/// Revoke the integration's current token(s) and issue a new one (keeps the same
/// `name`). Returns the new plaintext token.
pub async fn rotate_api_token(db: &SqlitePool, principal_id: &str) -> sqlx::Result<String> {
    let name: Option<(String,)> =
        sqlx::query_as("SELECT display_name FROM principals WHERE id = ?1 AND kind = 'bot'")
            .bind(principal_id)
            .fetch_optional(db)
            .await?;
    let name = name.map(|(n,)| n).unwrap_or_default();
    revoke_api_tokens(db, principal_id).await?;
    issue_api_token(db, principal_id, &name).await
}

/// Resolve an API token to its (bot) principal, if active.
pub async fn resolve_api_token(reads: &SqlitePool, token: &str) -> sqlx::Result<Option<Principal>> {
    sqlx::query_as::<_, Principal>(
        "SELECT p.id, p.kind, p.display_name, p.is_admin, p.avatar
         FROM api_tokens t JOIN principals p ON p.id = t.principal_id
         WHERE t.token_hash = ?1 AND t.revoked_at IS NULL",
    )
    .bind(hash_token(token))
    .fetch_optional(reads)
    .await
}

/// Best-effort `last_used_at` touch (called after a successful API auth).
pub async fn touch_api_token(db: &SqlitePool, token: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE api_tokens SET last_used_at = ?1 WHERE token_hash = ?2")
        .bind(now_millis())
        .bind(hash_token(token))
        .execute(db)
        .await?;
    Ok(())
}

pub async fn list_integrations(reads: &SqlitePool) -> sqlx::Result<Vec<IntegrationSummary>> {
    sqlx::query_as::<_, IntegrationSummary>(
        "SELECT p.id, p.display_name AS name, p.created_at,
                (SELECT MAX(t.last_used_at) FROM api_tokens t WHERE t.principal_id = p.id)
                  AS last_used_at,
                EXISTS(SELECT 1 FROM api_tokens t
                       WHERE t.principal_id = p.id AND t.revoked_at IS NULL) AS active
         FROM principals p
         WHERE p.kind = 'bot'
         ORDER BY p.created_at DESC",
    )
    .fetch_all(reads)
    .await
}

// ---- sessions --------------------------------------------------------------

/// Create a cookie session for a principal; returns the plaintext session token.
pub async fn create_session(db: &SqlitePool, principal_id: &str) -> sqlx::Result<String> {
    let token = new_token();
    let now = now_millis();
    sqlx::query(
        "INSERT INTO sessions (token_hash, principal_id, created_at, expires_at, last_seen)
         VALUES (?1, ?2, ?3, ?4, ?3)",
    )
    .bind(hash_token(&token))
    .bind(principal_id)
    .bind(now)
    .bind(now + SESSION_TTL_MS)
    .execute(db)
    .await?;
    Ok(token)
}

async fn lookup_session(reads: &SqlitePool, token: &str) -> sqlx::Result<Option<Principal>> {
    sqlx::query_as::<_, Principal>(
        "SELECT p.id, p.kind, p.display_name, p.is_admin, p.avatar
         FROM sessions s JOIN principals p ON p.id = s.principal_id
         WHERE s.token_hash = ?1 AND s.expires_at > ?2",
    )
    .bind(hash_token(token))
    .bind(now_millis())
    .fetch_optional(reads)
    .await
}

pub async fn delete_session(db: &SqlitePool, token: &str) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = ?1")
        .bind(hash_token(token))
        .execute(db)
        .await?;
    Ok(())
}

/// The current identity from a request's cookie jar, if any (no rejection).
pub async fn principal_from_jar(state: &AppState, jar: &CookieJar) -> Option<Principal> {
    let token = jar.get(SESSION_COOKIE)?.value().to_string();
    lookup_session(&state.reads, &token).await.ok().flatten()
}

// ---- cookies ---------------------------------------------------------------

pub fn session_cookie(value: String, secure: bool) -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, value))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/")
        .secure(secure)
        .max_age(time::Duration::milliseconds(SESSION_TTL_MS))
        .build()
}

pub fn clear_cookie(secure: bool) -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, ""))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/")
        .secure(secure)
        .max_age(time::Duration::ZERO)
        .build()
}

// ---- extractors ------------------------------------------------------------

impl FromRequestParts<AppState> for Identity {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_headers(&parts.headers);
        let token = jar
            .get(SESSION_COOKIE)
            .map(|c| c.value().to_string())
            .ok_or(StatusCode::UNAUTHORIZED)?;
        match lookup_session(&state.reads, &token).await {
            Ok(Some(p)) => Ok(Identity(p)),
            Ok(None) => Err(StatusCode::UNAUTHORIZED),
            Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
}

impl FromRequestParts<AppState> for Admin {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let Identity(p) = Identity::from_request_parts(parts, state).await?;
        if p.is_admin {
            Ok(Admin(p))
        } else {
            Err(StatusCode::FORBIDDEN)
        }
    }
}

/// Pull the bearer token out of the `Authorization` header.
fn bearer_token(parts: &Parts) -> Option<String> {
    let value = parts.headers.get("authorization")?.to_str().ok()?;
    value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

impl FromRequestParts<AppState> for ApiAuth {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = bearer_token(parts).ok_or(StatusCode::UNAUTHORIZED)?;
        // Per-token rate limit (keyed by the hash so no plaintext lands in the map).
        if !state.limits.api.check(&hash_token(&token)) {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        match resolve_api_token(&state.reads, &token).await {
            Ok(Some(p)) => {
                // Best-effort usage stamp; don't block the request on it.
                let db = state.db.clone();
                tokio::spawn(async move {
                    let _ = touch_api_token(&db, &token).await;
                });
                Ok(ApiAuth(p))
            }
            Ok(None) => Err(StatusCode::UNAUTHORIZED),
            Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    struct TestDb {
        write: SqlitePool,
        reads: SqlitePool,
        path: String,
    }

    impl Drop for TestDb {
        fn drop(&mut self) {
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{suffix}", self.path));
            }
        }
    }

    async fn temp_db() -> TestDb {
        let path = std::env::temp_dir()
            .join(format!("zen-test-{}.db", Ulid::new()))
            .to_string_lossy()
            .into_owned();
        let write = crate::db::open_writer(&path).await.unwrap();
        let reads = crate::db::open_readers(&path, 2).await.unwrap();
        TestDb { write, reads, path }
    }

    #[tokio::test]
    async fn token_resolves_then_revokes() {
        let db = temp_db().await;
        let p = create_principal(&db.write, "client", "anon", false)
            .await
            .unwrap();
        let token = issue_token(&db.write, &p.id, None).await.unwrap();

        let resolved = resolve_token(&db.reads, &token).await.unwrap();
        assert_eq!(resolved.map(|r| r.id), Some(p.id.clone()));

        revoke_tokens(&db.write, &p.id).await.unwrap();
        assert!(resolve_token(&db.reads, &token).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn rotate_invalidates_old_token() {
        let db = temp_db().await;
        let p = create_principal(&db.write, "user", "admin", true)
            .await
            .unwrap();
        let old = issue_token(&db.write, &p.id, None).await.unwrap();
        let new = rotate_token(&db.write, &p.id).await.unwrap();

        assert!(resolve_token(&db.reads, &old).await.unwrap().is_none());
        assert_eq!(
            resolve_token(&db.reads, &new).await.unwrap().map(|r| r.id),
            Some(p.id)
        );
    }

    #[tokio::test]
    async fn session_lifecycle() {
        let db = temp_db().await;
        let p = create_principal(&db.write, "user", "admin", true)
            .await
            .unwrap();
        let session = create_session(&db.write, &p.id).await.unwrap();

        let found = lookup_session(&db.reads, &session).await.unwrap();
        assert!(found.is_some());

        delete_session(&db.write, &session).await.unwrap();
        assert!(lookup_session(&db.reads, &session).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn unknown_token_does_not_resolve() {
        let db = temp_db().await;
        assert!(resolve_token(&db.reads, "nope").await.unwrap().is_none());
        assert!(lookup_session(&db.reads, "nope").await.unwrap().is_none());
        assert_eq!(count_principals(&db.reads).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn api_token_resolves_to_bot_then_rotates_and_revokes() {
        let db = temp_db().await;
        let (bot_id, token) = create_integration(&db.write, "CRM").await.unwrap();

        // Resolves to a bot principal and is prefixed.
        assert!(token.starts_with("zk_"));
        let p = resolve_api_token(&db.reads, &token).await.unwrap().unwrap();
        assert_eq!(p.id, bot_id);
        assert_eq!(p.kind, "bot");

        // Link-token resolution must not accept an API token (separate tables).
        assert!(resolve_token(&db.reads, &token).await.unwrap().is_none());

        // Rotation invalidates the old token and yields a working new one.
        let rotated = rotate_api_token(&db.write, &bot_id).await.unwrap();
        assert!(resolve_api_token(&db.reads, &token)
            .await
            .unwrap()
            .is_none());
        assert!(resolve_api_token(&db.reads, &rotated)
            .await
            .unwrap()
            .is_some());

        // Revocation kills it entirely.
        revoke_api_tokens(&db.write, &bot_id).await.unwrap();
        assert!(resolve_api_token(&db.reads, &rotated)
            .await
            .unwrap()
            .is_none());

        // Bots are excluded from the link-management list.
        assert!(list_principals(&db.reads).await.unwrap().is_empty());
        // …but appear as an integration.
        let ints = list_integrations(&db.reads).await.unwrap();
        assert_eq!(ints.len(), 1);
        assert_eq!(ints[0].name, "CRM");
        assert!(!ints[0].active); // we just revoked
    }
}
