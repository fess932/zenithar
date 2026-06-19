use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::Router;
use tokio::sync::broadcast;
use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::EnvFilter;

mod db;
mod models;
mod writer;
mod ws;

use ws::AppState;

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

/// Unix time in milliseconds — our message timestamp unit.
pub fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let db_path = std::env::var("ZENITHAR_DB").unwrap_or_else(|_| "data/zenithar.db".to_string());
    let bind = std::env::var("ZENITHAR_BIND").unwrap_or_else(|_| "127.0.0.1:3000".to_string());

    // Ensure the (git-ignored) data dir exists before SQLite opens the file.
    if let Some(parent) = std::path::Path::new(&db_path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    // Writer uses a single-connection pool; readers get their own pool.
    let write_pool = db::open_writer(&db_path).await?;
    let reads = db::open_readers(&db_path, 4).await?;

    let (write_tx, write_rx) = writer::channel();
    tokio::spawn(writer::run(write_pool, write_rx));

    let (broadcast_tx, _) = broadcast::channel::<String>(256);

    let state = AppState {
        writes: write_tx,
        broadcast: broadcast_tx,
        reads,
    };

    let app = Router::new()
        .route("/ws", any(ws::ws_handler))
        .route("/api/health", get(|| async { "ok" }))
        .fallback(static_handler)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    info!(%db_path, "zenithar backend listening on http://{bind}");
    axum::serve(listener, app).await?;
    Ok(())
}
