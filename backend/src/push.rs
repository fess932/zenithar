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
//! Disabled unless `ZENITHAR_FCM_CREDENTIALS` points at the service-account JSON;
//! a self-host without it runs exactly as before (no push).

use std::sync::Mutex;

use anyhow::{Context, Result};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};

const SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";

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
    /// Load from a service-account JSON file. Returns `Ok(None)` if `path` is
    /// empty (feature off); `Err` only if a path is given but unusable.
    pub fn from_env(path: Option<String>) -> Result<Option<Self>> {
        let Some(path) = path.filter(|p| !p.is_empty()) else {
            return Ok(None);
        };
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading FCM credentials at {path}"))?;
        let sa: ServiceAccount =
            serde_json::from_str(&raw).context("parsing FCM service-account JSON")?;
        let key = EncodingKey::from_rsa_pem(sa.private_key.as_bytes())
            .context("FCM private_key is not a valid RSA PEM")?;
        Ok(Some(Self {
            project_id: sa.project_id,
            client_email: sa.client_email,
            token_uri: sa.token_uri,
            key,
            http: reqwest::Client::new(),
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

        let resp = self
            .http
            .post(&self.token_uri)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &jwt),
            ])
            .send()
            .await
            .context("FCM token request failed")?;
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

        let resp = self
            .http
            .post(&url)
            .bearer_auth(&access)
            .json(&payload)
            .send()
            .await
            .context("FCM send failed")?;

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
