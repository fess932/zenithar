//! Firebase Cloud Messaging (FCM, HTTP v1) sender for offline push.
//!
//! When a message lands for a room, [`crate::send::deliver`] pushes to members
//! who have no live WebSocket (they're backgrounded/closed on a phone). FCM is
//! the only transport that reaches a sleeping Android app without our own
//! always-on service.
//!
//! Auth is a service account: we sign a short-lived JWT (RS256) with the
//! account's private key and exchange it for an OAuth2 access token, cached
//! until just before it expires. No Google SDK — just HTTPS + a JWT.
//!
//! Push is enabled by dropping the service-account JSON at
//! [`DEFAULT_CREDENTIALS_PATH`] (`/data/fcm-sa.json` — inside the mounted data
//! volume). No env needed; `ZENITHAR_FCM_CREDENTIALS` only overrides the path
//! (handy for local dev). The file's absence is logged and the server runs
//! exactly as before (no push).

use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};

const SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";

/// Where we look for the FCM service-account JSON by default. Sits inside the
/// container's mounted `/data` volume, so a self-host just drops the file there.
pub const DEFAULT_CREDENTIALS_PATH: &str = "/data/fcm-sa.json";

/// The bits of a Google service-account JSON we use.
#[derive(Deserialize)]
struct ServiceAccount {
    project_id: String,
    client_email: String,
    private_key: String,
    #[serde(default = "default_token_uri")]
    token_uri: String,
}

fn default_token_uri() -> String {
    "https://oauth2.googleapis.com/token".to_string()
}

/// Claims for the JWT we exchange for an access token.
#[derive(Serialize)]
struct Claims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    iat: i64,
    exp: i64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: i64,
}

/// A configured FCM sender. Cheap to clone behind an `Arc` (held in `AppState`).
pub struct Fcm {
    project_id: String,
    client_email: String,
    token_uri: String,
    key: EncodingKey,
    http: reqwest::Client,
    /// Cached OAuth token: `(access_token, expiry_unix_millis)`.
    cached: Mutex<Option<(String, i64)>>,
}

impl Fcm {
    /// Load the service-account JSON from `path`. `Ok(None)` means the file isn't
    /// there yet (push simply off); `Err` means it's present but broken (bad JSON
    /// or key) — worth surfacing loudly.
    pub fn load(path: &str) -> Result<Option<Self>> {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("reading FCM credentials at {path}")),
        };
        let sa: ServiceAccount =
            serde_json::from_str(&raw).context("parsing FCM service-account JSON")?;
        let key = EncodingKey::from_rsa_pem(sa.private_key.as_bytes())
            .context("FCM private_key is not a valid RSA PEM")?;
        Ok(Some(Self {
            project_id: sa.project_id,
            client_email: sa.client_email,
            token_uri: sa.token_uri,
            key,
            // Explicit timeouts so a stalled Google connection (RU networks throttle
            // them) fails fast and lets the retry kick in, instead of hanging.
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            cached: Mutex::new(None),
        }))
    }

    /// A valid OAuth2 access token, minting + caching a fresh one when the cached
    /// one is missing or within 60s of expiry.
    async fn access_token(&self) -> Result<String> {
        let now = crate::now_millis();
        if let Some((tok, exp)) = self.cached.lock().unwrap().as_ref() {
            if *exp - now > 60_000 {
                return Ok(tok.clone());
            }
        }

        let iat = now / 1000;
        let claims = Claims {
            iss: &self.client_email,
            scope: SCOPE,
            aud: &self.token_uri,
            iat,
            exp: iat + 3600,
        };
        let jwt = jsonwebtoken::encode(&Header::new(Algorithm::RS256), &claims, &self.key)
            .context("signing FCM JWT")?;

        let resp = send_with_retry(
            || {
                self.http.post(&self.token_uri).form(&[
                    ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                    ("assertion", &jwt),
                ])
            },
            "FCM token request failed",
        )
        .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("FCM token exchange {status}: {body}");
        }
        let tr: TokenResponse = resp.json().await.context("parsing FCM token response")?;
        let exp = now + tr.expires_in.max(0) * 1000;
        *self.cached.lock().unwrap() = Some((tr.access_token.clone(), exp));
        Ok(tr.access_token)
    }

    /// Send one notification. `Ok(true)` = accepted; `Ok(false)` = the token is
    /// dead and the caller should delete it; `Err` = transient/unknown failure.
    pub async fn send(&self, token: &str, title: &str, body: &str, room_id: &str) -> Result<bool> {
        let access = self.access_token().await?;
        let url = format!(
            "https://fcm.googleapis.com/v1/projects/{}/messages:send",
            self.project_id
        );
        let payload = serde_json::json!({
            "message": {
                "token": token,
                "notification": { "title": title, "body": body },
                // Carried into the tap-to-open intent so the app can deep-link
                // straight to the room.
                "data": { "room_id": room_id },
                "android": { "priority": "high" }
            }
        });

        let resp = send_with_retry(
            || self.http.post(&url).bearer_auth(&access).json(&payload),
            "FCM send failed",
        )
        .await?;

        if resp.status().is_success() {
            return Ok(true);
        }
        // 404 = the token was never registered or was unregistered; 400 with an
        // UNREGISTERED/invalid-token body means the same. Either way: prune it.
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if status.as_u16() == 404 || text.contains("UNREGISTERED") {
            return Ok(false);
        }
        anyhow::bail!("FCM send {status}: {text}")
    }
}

/// Send a request, retrying transient transport failures (timeout/connection) a
/// couple of times with a short backoff. Google endpoints are flaky from some
/// networks and a dropped push isn't fatal, so best-effort is fine. Only the
/// `.send()` transport error is retried — a returned HTTP status is handled by
/// the caller (a 401/404 wouldn't be fixed by retrying).
async fn send_with_retry<F>(make: F, ctx: &'static str) -> Result<reqwest::Response>
where
    F: Fn() -> reqwest::RequestBuilder,
{
    const RETRIES: u32 = 2;
    let mut attempt = 0u32;
    loop {
        match make().send().await {
            Ok(resp) => return Ok(resp),
            Err(e) if attempt < RETRIES => {
                attempt += 1;
                tracing::debug!(attempt, ctx, error = %e, "FCM request failed — retrying");
                tokio::time::sleep(Duration::from_millis(200 * u64::from(attempt))).await;
            }
            Err(e) => return Err(e).context(ctx),
        }
    }
}
