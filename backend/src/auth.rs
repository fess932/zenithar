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

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct Principal {
    pub id: String,
    pub kind: String, // "user" | "client"
    pub display_name: String,
    pub is_admin: bool,
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
    })
}

pub async fn count_principals(reads: &SqlitePool) -> sqlx::Result<i64> {
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM principals")
        .fetch_one(reads)
        .await?;
    Ok(n)
}

pub async fn list_principals(reads: &SqlitePool) -> sqlx::Result<Vec<PrincipalSummary>> {
    sqlx::query_as::<_, PrincipalSummary>(
        "SELECT p.id, p.kind, p.display_name, p.is_admin, p.created_at,
                EXISTS(SELECT 1 FROM tokens t
                       WHERE t.principal_id = p.id AND t.revoked_at IS NULL) AS active
         FROM principals p
         ORDER BY p.created_at DESC",
    )
    .fetch_all(reads)
    .await
}

pub async fn set_display_name(db: &SqlitePool, principal_id: &str, name: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE principals SET display_name = ?1 WHERE id = ?2")
        .bind(name)
        .bind(principal_id)
        .execute(db)
        .await?;
    Ok(())
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
        "SELECT p.id, p.kind, p.display_name, p.is_admin
         FROM tokens t JOIN principals p ON p.id = t.principal_id
         WHERE t.token_hash = ?1 AND t.revoked_at IS NULL",
    )
    .bind(hash_token(token))
    .fetch_optional(reads)
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
        "SELECT p.id, p.kind, p.display_name, p.is_admin
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
}
