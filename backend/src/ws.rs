use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast::error::RecvError;
use tracing::{debug, info};
use ulid::Ulid;

use crate::auth::{Identity, Principal};
use crate::db;
use crate::models::{ChatMessage, Inbound, Outbound};
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
    let is_employee = principal.kind == "user";

    // Clients are pinned to their own room; employees default to common.
    let client_room = if is_employee {
        None
    } else {
        match db::ensure_client_room(&state.db, &principal.id).await {
            Ok(id) => Some(id),
            Err(e) => {
                debug!(error = %e, "could not resolve client room");
                return;
            }
        }
    };
    let mut active_room = client_room
        .clone()
        .unwrap_or_else(|| COMMON_ROOM.to_string());

    let mut rx = state.broadcast.subscribe();
    let (mut sender, mut receiver) = socket.split();

    // Initial transcript for the default room.
    if send_history(&mut sender, &state, &active_room)
        .await
        .is_err()
    {
        return;
    }

    loop {
        tokio::select! {
            biased;

            // Inbound from this socket.
            inbound = receiver.next() => {
                let Some(Ok(msg)) = inbound else { break };
                let text = match msg {
                    Message::Text(t) => t,
                    Message::Close(_) => break,
                    _ => continue,
                };
                let frame = match serde_json::from_str::<Inbound>(&text) {
                    Ok(f) => f,
                    Err(_) => { debug!("dropping unparseable client frame"); continue }
                };

                match frame {
                    Inbound::Join { room_id } => {
                        // Employees may open any existing room; clients stay pinned.
                        let allowed = match &client_room {
                            Some(room) => room_id == *room,
                            None => db::room_exists(&state.reads, &room_id).await.unwrap_or(false),
                        };
                        if !allowed {
                            debug!(room = %room_id, "join denied");
                            continue;
                        }
                        active_room = room_id;
                        if send_history(&mut sender, &state, &active_room).await.is_err() {
                            break;
                        }
                    }
                    Inbound::Msg { body, client_msg_id } => {
                        let chat = ChatMessage {
                            id: Ulid::new().to_string(),
                            room_id: active_room.clone(),
                            author_id: principal.id.clone(),
                            author_name: principal.display_name.clone(),
                            body,
                            client_msg_id,
                            created_at: crate::now_millis(),
                        };
                        // Realtime first: fan out to everyone subscribed.
                        let _ = state.broadcast.send(chat.clone());
                        // Durability second: batched write.
                        if state.writes.send(WriteCmd { msg: chat, ack: None }).await.is_err() {
                            break; // writer gone
                        }
                    }
                }
            }

            // Broadcast fan-out: forward only the active room (no cross-room leak).
            bcast = rx.recv() => {
                match bcast {
                    Ok(chat) if chat.room_id == active_room => {
                        let frame = Outbound::Message { message: chat };
                        if let Ok(json) = serde_json::to_string(&frame) {
                            if sender.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Ok(_) => {}                       // different room → ignore
                    Err(RecvError::Lagged(_)) => {}   // dropped some; keep going
                    Err(RecvError::Closed) => break,
                }
            }
        }
    }
    info!("websocket closed");
}

/// Send a room's recent transcript as a single `history` frame.
async fn send_history(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    state: &AppState,
    room_id: &str,
) -> Result<(), ()> {
    let messages = match db::recent_messages(&state.reads, room_id, HISTORY_ON_CONNECT).await {
        Ok(m) => m,
        Err(e) => {
            debug!(error = %e, "failed to load history");
            Vec::new()
        }
    };
    let frame = Outbound::History {
        room_id: room_id.to_string(),
        messages,
    };
    let json = serde_json::to_string(&frame).map_err(|_| ())?;
    sender
        .send(Message::Text(json.into()))
        .await
        .map_err(|_| ())
}
