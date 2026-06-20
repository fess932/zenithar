//! Admin-gated reverse proxy for the GreptimeDB dashboard, so telemetry lives on
//! the main origin (`/otel/...`) instead of a separate `:4000` — and behind our
//! session cookie (GreptimeDB itself has no auth). Only an admin's browser, which
//! carries the session cookie, gets through the [`Admin`] extractor; every asset
//! and API call the dashboard makes is same-origin, so the cookie rides along.
//!
//! The dashboard is an SPA, so we (a) inject a `<base href="/otel/">` into its
//! HTML so relative assets resolve under the prefix, and (b) rewrite root-relative
//! `Location` redirects back under `/otel`. Requests are forwarded to GreptimeDB's
//! HTTP port, derived from `ZENITHAR_OTLP_ENDPOINT`.

use std::sync::OnceLock;

use axum::body::{to_bytes, Bytes};
use axum::extract::Request;
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::auth::Admin;

const PREFIX: &str = "/otel";
const MAX_BODY: usize = 32 * 1024 * 1024;

/// `ANY /otel/{*path}` — forward to the GreptimeDB dashboard. Admin only.
pub async fn proxy(_admin: Admin, req: Request) -> Response {
    let base = greptime_base();
    let path = req.uri().path();
    let rest = path.strip_prefix(PREFIX).unwrap_or(path);
    let rest = if rest.is_empty() { "/" } else { rest };
    let query = req
        .uri()
        .query()
        .map(|q| format!("?{q}"))
        .unwrap_or_default();
    let url = format!("{base}{rest}{query}");

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
        Err(_) => return (StatusCode::BAD_GATEWAY, "telemetry backend unavailable").into_response(),
    };

    let status = resp.status();
    let mut out = HeaderMap::new();
    let mut content_type = String::new();
    for (k, v) in resp.headers().iter() {
        if is_hop(k) || k == header::CONTENT_LENGTH || k == header::TRANSFER_ENCODING {
            continue;
        }
        if k == header::LOCATION {
            // Keep redirects inside the /otel prefix.
            if let Ok(s) = v.to_str() {
                let nv = if s.starts_with('/') {
                    format!("{PREFIX}{s}")
                } else {
                    s.to_string()
                };
                if let Ok(hv) = HeaderValue::from_str(&nv) {
                    out.insert(header::LOCATION, hv);
                }
            }
            continue;
        }
        if k == header::CONTENT_TYPE {
            content_type = v.to_str().unwrap_or("").to_string();
        }
        out.append(k.clone(), v.clone());
    }

    let bytes = resp.bytes().await.unwrap_or_default();
    let body = if content_type.starts_with("text/html") {
        let html = String::from_utf8_lossy(&bytes);
        Bytes::from(html.replacen("<head>", &format!("<head><base href=\"{PREFIX}/\">"), 1))
    } else {
        bytes
    };

    (status, out, body).into_response()
}

fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            // Pass redirects through to the browser (rewritten) instead of following.
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
