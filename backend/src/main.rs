use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use axum::extract::DefaultBodyLimit;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::Router;
use tokio::sync::broadcast;
use tower_http::trace::TraceLayer;
use tracing::info;

mod api;
mod auth;
mod calls;
mod dashproxy;
mod db;
mod models;
mod names;
mod presence;
mod ratelimit;
mod recordings;
mod routes;
mod send;
mod state;
mod storage;
mod telemetry;
mod uploads;
mod writer;
mod ws;

use state::AppState;

/// The built frontend, embedded into the binary for release builds. In debug
/// builds rust-embed reads from disk at runtime, so `bun run dev` changes show
/// up without recompiling the server.
#[derive(rust_embed::Embed)]
#[folder = "../frontend/dist"]
struct Assets;

/// Serve an embedded asset, falling back to index.html for unknown paths.
async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    serve_asset(path)
        .or_else(|| serve_asset("index.html"))
        .unwrap_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                "frontend not built — run `make fe-build`",
            )
                .into_response()
        })
}

fn serve_asset(path: &str) -> Option<Response> {
    let file = Assets::get(path)?;
    let mime = file.metadata.mimetype().to_owned();
    Some(([(header::CONTENT_TYPE, mime)], file.data.into_owned()).into_response())
}

/// The SPA shell — served after a successful link login (`/i/:token`).
pub fn index_html_response() -> Response {
    serve_asset("index.html")
        .unwrap_or_else(|| (StatusCode::NOT_FOUND, "frontend not built").into_response())
}

/// Unix time in milliseconds — our message timestamp unit.
pub fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Self-contained liveness probe: `zenithar-backend healthcheck` exits 0 when the
/// HTTP server answers `/api/health` with 200, else 1. This lets the shell-less
/// distroless image carry a Docker HEALTHCHECK without bundling curl/wget — the
/// The HTTP/WebSocket listen address. `ZENITHAR_BIND` sets the full `host:port`
/// (default `127.0.0.1:3000`; the image sets `0.0.0.0:3000`). `ZENITHAR_PORT` is a
/// convenience that overrides just the port — handy under host networking, where
/// the compose `ports:` mapping doesn't apply so the port can't be remapped there.
fn bind_addr() -> String {
    let bind = std::env::var("ZENITHAR_BIND").unwrap_or_else(|_| "127.0.0.1:3000".to_string());
    match std::env::var("ZENITHAR_PORT") {
        Ok(port) if !port.trim().is_empty() => {
            let host = bind.rsplit_once(':').map(|(h, _)| h).unwrap_or("0.0.0.0");
            format!("{host}:{}", port.trim())
        }
        _ => bind,
    }
}

/// one binary checks itself over loopback.
fn run_healthcheck() -> i32 {
    use std::io::{Read, Write};
    let bind = bind_addr();
    let port = bind.rsplit(':').next().unwrap_or("3000");
    let timeout = std::time::Duration::from_secs(3);
    let Ok(addr) = format!("127.0.0.1:{port}").parse::<std::net::SocketAddr>() else {
        return 1;
    };
    let Ok(mut stream) = std::net::TcpStream::connect_timeout(&addr, timeout) else {
        return 1;
    };
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    if stream
        .write_all(b"GET /api/health HTTP/1.0\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .is_err()
    {
        return 1;
    }
    let mut buf = String::new();
    let _ = stream.read_to_string(&mut buf);
    i32::from(!buf.lines().next().is_some_and(|l| l.contains(" 200")))
}

/// On first run (empty DB) create an admin and surface its login link, so
/// there's someone who can create the rest of the principals from the UI. The
/// link is logged and saved to `.env` (git-ignored) so it can't be lost.
async fn bootstrap_admin(db: &sqlx::SqlitePool, bind: &str) -> Result<()> {
    if auth::count_principals(db).await? > 0 {
        return Ok(());
    }
    let admin = auth::create_principal(db, "user", "admin", true).await?;
    let token = auth::issue_token(db, &admin.id, None).await?;
    let path = format!("/i/{token}");

    if let Err(e) = upsert_env("ZENITHAR_ADMIN_LINK", &path) {
        info!(error = %e, "could not write admin link to .env");
    }
    info!("first run — admin login link (open once, also saved to .env): http://{bind}{path}");
    Ok(())
}

/// Insert or replace a `KEY=value` line in the `.env` file in the current
/// working directory (created if missing). `.env` is git-ignored.
fn upsert_env(key: &str, value: &str) -> std::io::Result<()> {
    let path = std::path::Path::new(".env");
    let mut lines: Vec<String> = if path.exists() {
        std::fs::read_to_string(path)?
            .lines()
            .filter(|l| !l.starts_with(&format!("{key}=")))
            .map(str::to_string)
            .collect()
    } else {
        Vec::new()
    };
    lines.push(format!("{key}={value}"));
    std::fs::write(path, lines.join("\n") + "\n")
}

#[tokio::main]
async fn main() -> Result<()> {
    // Health-probe mode (used by the Docker HEALTHCHECK) — check and exit early.
    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        std::process::exit(run_healthcheck());
    }

    // Console logs always; OTLP trace export only if ZENITHAR_OTLP_ENDPOINT is set.
    // The provider lives for the whole process (never shut down) — see telemetry.
    telemetry::init();

    let db_path = std::env::var("ZENITHAR_DB").unwrap_or_else(|_| "data/zenithar.db".to_string());
    let bind = bind_addr();
    let secure_cookies = std::env::var("ZENITHAR_SECURE_COOKIES")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    // Ensure the (git-ignored) data dir exists before SQLite opens the file.
    let data_dir = std::path::Path::new(&db_path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    std::fs::create_dir_all(&data_dir)?;

    // Attachment blobs live next to the DB (git-ignored), behind the Storage trait.
    let attach_dir = std::env::var("ZENITHAR_ATTACHMENTS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| data_dir.join("attachments"));
    let storage: Arc<dyn storage::Storage> = Arc::new(storage::DiskStorage::new(attach_dir)?);

    // Writer uses a single-connection pool; readers get their own pool.
    let write_pool = db::open_writer(&db_path).await?;
    let reads = db::open_readers(&db_path, 4).await?;

    bootstrap_admin(&write_pool, &bind).await?;

    let (write_tx, write_rx) = writer::channel();
    tokio::spawn(writer::run(write_pool.clone(), write_rx));

    let (broadcast_tx, _) = broadcast::channel::<models::ChatMessage>(256);
    let (signal_tx, _) = broadcast::channel::<models::Signal>(256);
    let (notify_tx, _) = broadcast::channel::<models::ClientNotice>(256);

    // STUN servers for WebRTC ICE (comma-separated). Empty is fine on a LAN /
    // localhost (host candidates) — the server has a public IP, so no TURN.
    let stun: Vec<String> = std::env::var("ZENITHAR_STUN")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    // Public IP(s) to advertise as host candidates (NAT 1:1). Set this on a
    // self-hosted server behind NAT so remote browsers can actually reach the
    // media path — see docs/deploy.md. Empty = advertise only local candidates.
    let public_ips: Vec<String> = std::env::var("ZENITHAR_PUBLIC_IP")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    // Fixed UDP port range for call media (one socket per participant, bound
    // 0.0.0.0). Forward exactly this range in the NAT/DMZ. Empty = ephemeral.
    // Accepts a range ("51000-51200") or a bare port ("51000" → 51000-51000).
    let udp_ports: Option<(u16, u16)> = std::env::var("ZENITHAR_UDP_PORTS").ok().and_then(|v| {
        let v = v.trim();
        match v.split_once('-') {
            Some((a, b)) => Some((a.trim().parse().ok()?, b.trim().parse().ok()?)),
            None => {
                let p: u16 = v.parse().ok()?;
                Some((p, p))
            }
        }
    });
    // Call recordings (Phase 5): one Ogg/Opus file per participant, on disk.
    let recordings_dir = std::env::var("ZENITHAR_RECORDINGS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| data_dir.join("recordings"));
    std::fs::create_dir_all(&recordings_dir)?;

    let calls = Arc::new(calls::CallRegistry::new(
        stun,
        public_ips,
        udp_ports,
        signal_tx.clone(),
        write_pool.clone(),
        recordings_dir.clone(),
    )?);

    // No static ZENITHAR_PUBLIC_IP? Auto-discover it from an external echo service
    // and refresh periodically (handles a dynamic IP). Plain-HTTP service so it
    // works with our TLS-less HTTP client; override with ZENITHAR_PUBLIC_IP_SERVICE.
    if !calls.has_public_ip() {
        let reg = calls.clone();
        let service = std::env::var("ZENITHAR_PUBLIC_IP_SERVICE")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "http://api.ipify.org".to_string());
        // Re-check this often (seconds) so a router-reboot IP change is picked up.
        let interval = std::env::var("ZENITHAR_PUBLIC_IP_INTERVAL")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|&n| n >= 10)
            .unwrap_or(300);
        tokio::spawn(async move {
            loop {
                match calls::discover_public_ip(&service).await {
                    Some(ip) => reg.set_public_ips(vec![ip]), // only changes on diff
                    None => info!(%service, "public IP discovery failed (will retry)"),
                }
                tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
            }
        });
    }

    // Presence, seeded with persisted last-seen so a restart doesn't show dashes.
    let presence = Arc::new(presence::PresenceRegistry::new());
    if let Ok(map) = db::load_last_seen(&reads).await {
        presence.seed_last_seen(map);
    }
    // Periodically persist last-seen so it survives a restart/redeploy.
    {
        let presence = presence.clone();
        let db = write_pool.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                let _ = db::save_last_seen(&db, &presence.last_seen_map()).await;
            }
        });
    }

    let state = AppState {
        writes: write_tx,
        broadcast: broadcast_tx,
        reads,
        db: write_pool,
        storage,
        signal: signal_tx,
        calls,
        notify: notify_tx,
        presence,
        limits: Arc::new(ratelimit::Limits::default()),
        secure_cookies,
        recordings_dir,
    };

    let app = Router::new()
        .route("/ws", any(ws::ws_handler))
        .route("/i/{token}", get(routes::enter_link))
        .route("/api/me", get(routes::me))
        .route("/api/ice", get(routes::ice_servers))
        .route("/api/me/name", post(routes::rename))
        .route("/api/rooms", get(routes::rooms))
        .route("/api/rooms/{id}/messages", get(routes::room_messages))
        .route("/api/people", get(routes::people))
        .route(
            "/api/upload",
            post(uploads::upload).layer(DefaultBodyLimit::max(uploads::MAX_UPLOAD_BYTES + 1024)),
        )
        .route("/api/attachments/{id}", get(uploads::serve))
        .route("/api/attachments/{id}/thumb", get(uploads::serve_thumb))
        .route("/api/auth/logout", post(routes::logout))
        .route(
            "/api/principals",
            get(routes::list_principals).post(routes::create_principal),
        )
        .route("/api/principals/{id}/rotate", post(routes::rotate))
        .route("/api/principals/{id}/revoke", post(routes::revoke))
        .route(
            "/api/integrations",
            get(routes::list_integrations).post(routes::create_integration),
        )
        .route(
            "/api/integrations/{id}/rotate",
            post(routes::rotate_integration),
        )
        .route(
            "/api/integrations/{id}/revoke",
            post(routes::revoke_integration),
        )
        // Admin: telemetry dashboard link + saved call recordings.
        .route("/api/admin/telemetry", get(routes::telemetry_info))
        .route("/api/admin/recordings", get(recordings::list))
        .route(
            "/api/admin/recordings/{call_id}/{participant_id}",
            get(recordings::serve),
        )
        // REST API for integrations (Bearer zk_… auth).
        .route("/api/v1/me", get(api::me))
        .route("/api/v1/rooms", get(api::rooms))
        .route(
            "/api/v1/rooms/{id}/messages",
            get(api::get_messages).post(api::post_message),
        )
        .route("/api/v1/clients", post(api::create_client))
        .route(
            "/api/v1/clients/{client_id}/messages",
            post(api::post_client_message),
        )
        .route(
            "/api/v1/uploads",
            post(api::upload).layer(DefaultBodyLimit::max(uploads::MAX_UPLOAD_BYTES + 1024)),
        )
        .fallback(static_handler)
        // INFO-level request spans so every HTTP request becomes an exported
        // trace (the default is DEBUG, which the `info` filter drops — leaving
        // only call spans). Makes telemetry visible from ordinary browsing.
        .layer(
            TraceLayer::new_for_http().make_span_with(
                tower_http::trace::DefaultMakeSpan::new().level(tracing::Level::INFO),
            ),
        )
        // Added AFTER the trace layer so they're NOT traced (avoid telemetry
        // noise): the loopback health probe, and the GreptimeDB dashboard
        // reverse-proxy — whose own /dashboard*,/v1/* traffic would otherwise echo
        // back as traces. Mirrors GreptimeDB's native paths so the SPA resolves.
        .route("/api/health", get(|| async { "ok" }))
        .route("/dashboard", any(dashproxy::proxy))
        .route("/dashboard/", any(dashproxy::proxy))
        .route("/dashboard/{*path}", any(dashproxy::proxy))
        .route("/v1/{*path}", any(dashproxy::proxy))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    info!(%db_path, "zenithar backend listening on http://{bind}");
    axum::serve(listener, app).await?;
    Ok(())
}
