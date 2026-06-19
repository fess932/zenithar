use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use tracing::{debug, info};
use ulid::Ulid;

use crate::auth::{Identity, Principal};
use crate::db;
use crate::models::{ChatMessage, IncomingMessage};
use crate::state::AppState;
use crate::writer::WriteCmd;

const COMMON_ROOM: &str = "common";
const HISTORY_ON_CONNECT: i64 = 50;

/// `/ws` — requires an authenticated identity (employee or client). The author
/// of every message is taken from that identity, never from the client frame.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Identity(principal): Identity,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state, principal))
}

async fn handle_socket(socket: WebSocket, state: AppState, principal: Principal) {
    let mut rx = state.broadcast.subscribe();
    let (mut sender, mut receiver) = socket.split();

    // Send recent history so a freshly connected client has context.
    match db::recent_messages(&state.reads, COMMON_ROOM, HISTORY_ON_CONNECT).await {
        Ok(history) => {
            for m in history {
                if let Ok(json) = serde_json::to_string(&m) {
                    if sender.send(Message::Text(json.into())).await.is_err() {
                        return;
                    }
                }
            }
        }
        Err(e) => debug!(error = %e, "failed to load history"),
    }

    // Outbound: forward room broadcasts to this client.
    let mut send_task = tokio::spawn(async move {
        while let Ok(json) = rx.recv().await {
            if sender.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }
    });

    // Inbound: parse client messages, stamp identity, broadcast immediately
    // (optimistic), then enqueue for batched persistence.
    let writes = state.writes.clone();
    let bcast = state.broadcast.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            let text = match msg {
                Message::Text(t) => t,
                Message::Close(_) => break,
                _ => continue,
            };
            let Ok(incoming) = serde_json::from_str::<IncomingMessage>(&text) else {
                debug!("dropping unparseable client frame");
                continue;
            };

            let chat = ChatMessage {
                id: Ulid::new().to_string(),
                room_id: COMMON_ROOM.to_string(),
                author_id: principal.id.clone(),
                author_name: principal.display_name.clone(),
                body: incoming.body,
                client_msg_id: incoming.client_msg_id,
                created_at: crate::now_millis(),
            };

            // Realtime first: everyone in the room sees it without waiting on disk.
            if let Ok(json) = serde_json::to_string(&chat) {
                let _ = bcast.send(json);
            }
            // Durability second: batched write.
            if writes
                .send(WriteCmd {
                    msg: chat,
                    ack: None,
                })
                .await
                .is_err()
            {
                break; // writer gone
            }
        }
    });

    // If either direction ends, tear down the other.
    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }
    info!("websocket closed");
}
