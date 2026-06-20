//! Admin-gated reverse proxy for the GreptimeDB dashboard, so telemetry lives on
//! the main origin (behind our session cookie — GreptimeDB itself has no auth)
//! instead of a separate `:4000`. Only an admin's browser, which carries the
//! session cookie, gets through the [`Admin`] extractor.
//!
//! The dashboard is an SPA with RELATIVE asset paths (`assets/…`) served under
//! `/dashboard/`, and it calls GreptimeDB's HTTP API under `/v1/…`. A custom URL
//! prefix would break both (relative assets + absolute API). So instead of
//! translating paths we MIRROR GreptimeDB's own paths on our origin: mount this
//! at `/dashboard*` and `/v1/*` and forward the path unchanged — everything
//! resolves the way the SPA expects, no `<base>` rewriting needed. Requests go to
//! GreptimeDB's HTTP port, derived from `ZENITHAR_OTLP_ENDPOINT`.

use std::sync::OnceLock;

use axum::body::{to_bytes, Bytes};
use axum::extract::Request;
use axum::http::{header, HeaderMap, HeaderName, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::auth::Admin;

const MAX_BODY: usize = 32 * 1024 * 1024;

/// `ANY /dashboard*` and `/v1/*` — forward to GreptimeDB unchanged. Admin only.
pub async fn proxy(_admin: Admin, req: Request) -> Response {
    let base = greptime_base();
    let path = req.uri().path();
    let query = req
        .uri()
        .query()
        .map(|q| format!("?{q}"))
        .unwrap_or_default();
    let url = format!("{base}{path}{query}");

    let method = req.method().clone();
    let headers = req.headers().clone();
    let Ok(body) = to_bytes(req.into_body(), MAX_BODY).await else {
        return StatusCode::BAD_REQUEST.into_response();
    };

    let mut rb = client().request(method, &url).body(body);
    for (k, v) in headers.iter() {
        // Host/hop-by-hop are connection-specific; reqwest sets its own.
        if k == header::HOST || is_hop(k) {
            continue;
        }
        rb = rb.header(k, v);
    }

    let resp = match rb.send().await {
        Ok(r) => r,
        Err(_) => {
            return (StatusCode::BAD_GATEWAY, "telemetry backend unavailable").into_response()
        }
    };

    let status = resp.status();
    let mut out = HeaderMap::new();
    for (k, v) in resp.headers().iter() {
        // Let axum set framing; copy everything else (incl. content-type, location).
        if is_hop(k) || k == header::CONTENT_LENGTH || k == header::TRANSFER_ENCODING {
            continue;
        }
        out.append(k.clone(), v.clone());
    }
    let bytes: Bytes = resp.bytes().await.unwrap_or_default();

    (status, out, bytes).into_response()
}

fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            // Pass redirects through to the browser (paths are already correct).
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_default()
    })
}

/// Scheme + authority of the GreptimeDB HTTP endpoint, from the OTLP endpoint.
fn greptime_base() -> String {
    std::env::var("ZENITHAR_OTLP_ENDPOINT")
        .ok()
        .and_then(|e| {
            let (scheme, rest) = e.split_once("://")?;
            let authority = rest.split('/').next()?;
            Some(format!("{scheme}://{authority}"))
        })
        .unwrap_or_else(|| "http://127.0.0.1:4000".to_string())
}

fn is_hop(k: &HeaderName) -> bool {
    matches!(
        k.as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailers"
            | "upgrade"
    )
}
