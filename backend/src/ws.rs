use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast::error::RecvError;
use tracing::{debug, info};
use ulid::Ulid;

use crate::auth::{Identity, Principal};
use crate::db;
use crate::models::{ChatMessage, Inbound, Outbound, Signal};
use crate::ratelimit::LocalBucket;
use crate::state::AppState;

const COMMON_ROOM: &str = "common";
const HISTORY_ON_CONNECT: i64 = 50;
const MAX_ATTACHMENTS: usize = 5;

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
    let mut sigrx = state.signal.subscribe();
    let mut notrx = state.notify.subscribe();
    let (mut sender, mut receiver) = socket.split();

    // Initial transcript for the default room.
    if send_history(&mut sender, &state, &active_room)
        .await
        .is_err()
    {
        return;
    }

    // Presence: mark online (others get a delta), then hand this socket the
    // current roster. Subscribe after join so we don't echo our own delta.
    state.presence.join(&principal.id, &principal.kind);
    let mut presrx = state.presence.subscribe();
    {
        let frame = Outbound::PresenceSnapshot {
            online: state.presence.snapshot(),
        };
        if let Ok(json) = serde_json::to_string(&frame) {
            if sender.send(Message::Text(json.into())).await.is_err() {
                state.presence.leave(&principal.id);
                return;
            }
        }
    }

    // Unread badges. Accessible rooms: clients see only their own, employees all.
    // On the first ever connect, baseline every room to "read" so old history
    // isn't shown as unread; then send the current per-room counts (they survive
    // a reload). The open room is marked read here and on every later switch.
    let accessible: Vec<String> = match &client_room {
        Some(room) => vec![room.clone()],
        None => db::list_rooms_for_user(&state.reads, &principal.id)
            .await
            .map(|v| v.into_iter().map(|r| r.id).collect())
            .unwrap_or_default(),
    };
    {
        let now = crate::now_millis();
        if !db::has_reads(&state.reads, &principal.id)
            .await
            .unwrap_or(true)
        {
            for room in &accessible {
                let _ = db::mark_read(&state.db, &principal.id, room, now).await;
            }
        }
        let _ = db::mark_read(&state.db, &principal.id, &active_room, now).await;
        if let Ok(counts) = db::unread_counts(&state.reads, &principal.id, &accessible).await {
            let frame = Outbound::UnreadCounts { counts };
            if let Ok(json) = serde_json::to_string(&frame) {
                if sender.send(Message::Text(json.into())).await.is_err() {
                    state.presence.leave(&principal.id);
                    return;
                }
            }
        }
    }
    // Rooms this principal may see — gates cross-room unread bumps so a private
    // DM never leaks to a non-member. Refreshed when a `rooms-changed` nudge
    // arrives (e.g. someone opens a DM with us mid-session).
    let mut accessible: std::collections::HashSet<String> = accessible.into_iter().collect();

    // Per-socket message rate limit: burst 10, ~1/s sustained.
    let mut msg_bucket = LocalBucket::new(10.0, 1.0);

    // Periodic WS ping to measure round-trip latency (the connections list shows
    // it). The browser auto-replies with a pong echoing our timestamp payload.
    let mut ping_iv = tokio::time::interval(std::time::Duration::from_secs(25));
    ping_iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            biased;

            // Inbound from this socket.
            inbound = receiver.next() => {
                let Some(Ok(msg)) = inbound else { break };
                // Any frame counts as activity (last-seen for the connections list).
                state.presence.touch(&principal.id);
                let text = match msg {
                    Message::Text(t) => t,
                    Message::Pong(payload) => {
                        // Pong echoes our ping's timestamp → round-trip = now - ts.
                        if payload.len() >= 8 {
                            let ts = i64::from_le_bytes(payload[..8].try_into().unwrap());
                            state
                                .presence
                                .set_ping(&principal.id, (crate::now_millis() - ts).max(0));
                        }
                        continue;
                    }
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
                            None => db::staff_can_open(&state.reads, &principal.id, &room_id)
                                .await
                                .unwrap_or(false),
                        };
                        if !allowed {
                            debug!(room = %room_id, "join denied");
                            continue;
                        }
                        // Leaving the current room → it's read up to now; the room
                        // we're opening becomes read too (clears its unread badge).
                        let now = crate::now_millis();
                        let _ = db::mark_read(&state.db, &principal.id, &active_room, now).await;
                        active_room = room_id;
                        // Opening a room (e.g. a DM we just started) adds it to the
                        // accessible set, so its unread bumps reach us afterwards.
                        accessible.insert(active_room.clone());
                        if send_history(&mut sender, &state, &active_room).await.is_err() {
                            break;
                        }
                        let _ = db::mark_read(&state.db, &principal.id, &active_room, now).await;
                    }
                    Inbound::Msg { body, client_msg_id, attachment_ids, reply_to } => {
                        if !msg_bucket.check() {
                            debug!("message rate-limited");
                            continue;
                        }
                        // Resolve up to 5 attachments, each must belong to this room.
                        let mut attachments = Vec::new();
                        let mut bad = false;
                        for aid in attachment_ids.into_iter().take(MAX_ATTACHMENTS) {
                            match db::lookup_attachment(&state.reads, &aid).await {
                                Ok(Some((room, att))) if room == active_room => attachments.push(att),
                                _ => { bad = true; break }
                            }
                        }
                        if bad {
                            debug!("dropping msg with bad attachment");
                            continue;
                        }
                        // Ignore empty messages that carry nothing.
                        if body.trim().is_empty() && attachments.is_empty() {
                            continue;
                        }
                        // Resolve the quoted message (must be in this room); a
                        // dangling reference is silently dropped, not rejected.
                        let reply_to = match reply_to {
                            Some(rid) => db::reply_preview(&state.reads, &rid, &active_room)
                                .await
                                .unwrap_or(None),
                            None => None,
                        };
                        let chat = ChatMessage {
                            id: Ulid::new().to_string(),
                            room_id: active_room.clone(),
                            author_id: principal.id.clone(),
                            author_name: principal.display_name.clone(),
                            author_avatar: principal.avatar.clone(),
                            body,
                            reply_to,
                            client_msg_id,
                            created_at: crate::now_millis(),
                            edited_at: None,
                            attachments,
                            reactions: Vec::new(),
                        };
                        // Fan out + (for anonymous clients) ping employees + write.
                        if crate::send::deliver(&state, chat, !is_employee).await.is_err() {
                            break; // writer gone
                        }
                    }
                    Inbound::Edit { id, body } => {
                        if !msg_bucket.check() {
                            debug!("edit rate-limited");
                            continue;
                        }
                        let body = body.trim().to_string();
                        if body.is_empty() || body.chars().count() > 4000 {
                            continue;
                        }
                        // Author only, and only within a room this socket may act in.
                        match db::message_meta(&state.reads, &id).await {
                            Ok(Some((room, author))) if author == principal.id => {
                                let in_room = client_room.as_deref().is_none_or(|r| r == room);
                                if in_room {
                                    let now = crate::now_millis();
                                    if db::edit_message(&state.db, &id, &body, now).await.is_ok() {
                                        let _ = state.signal.send(Signal {
                                            room_id: room.clone(),
                                            target: None,
                                            exclude: None,
                                            all_employees: false,
                                            frame: Outbound::MessageEdited {
                                                id,
                                                room_id: room,
                                                body,
                                                edited_at: now,
                                            },
                                        });
                                    }
                                }
                            }
                            _ => debug!(msg = %id, "edit denied or message gone"),
                        }
                    }
                    Inbound::Delete { id } => {
                        // Author OR any admin; clients only within their own room.
                        match db::message_meta(&state.reads, &id).await {
                            Ok(Some((room, author)))
                                if author == principal.id || principal.is_admin =>
                            {
                                let in_room = client_room.as_deref().is_none_or(|r| r == room);
                                if in_room && db::delete_message(&state.db, &id).await.is_ok() {
                                    let _ = state.signal.send(Signal {
                                        room_id: room.clone(),
                                        target: None,
                                        exclude: None,
                                        all_employees: false,
                                        frame: Outbound::MessageDeleted { id, room_id: room },
                                    });
                                }
                            }
                            _ => debug!(msg = %id, "delete denied or message gone"),
                        }
                    }
                    Inbound::React { id, emoji } => {
                        if !msg_bucket.check() {
                            debug!("react rate-limited");
                            continue;
                        }
                        // A single grapheme-ish emoji; reject empty / oversized input.
                        let emoji = emoji.trim().to_string();
                        if emoji.is_empty() || emoji.chars().count() > 8 {
                            continue;
                        }
                        // Anyone in the room may react (not just the author).
                        match db::message_meta(&state.reads, &id).await {
                            Ok(Some((room, author))) => {
                                let in_room = client_room.as_deref().is_none_or(|r| r == room);
                                if !in_room {
                                    continue;
                                }
                                let added = match db::toggle_reaction(
                                    &state.db,
                                    &id,
                                    &principal.id,
                                    &emoji,
                                    crate::now_millis(),
                                )
                                .await
                                {
                                    Ok(a) => a,
                                    Err(_) => {
                                        debug!(msg = %id, "react: toggle failed");
                                        continue;
                                    }
                                };
                                // Live chip update for everyone viewing the room.
                                let reactions = db::reactions_for_message(&state.reads, &id)
                                    .await
                                    .unwrap_or_default();
                                let _ = state.signal.send(Signal {
                                    room_id: room.clone(),
                                    target: None,
                                    exclude: None,
                                    all_employees: false,
                                    frame: Outbound::MessageReaction {
                                        id: id.clone(),
                                        room_id: room.clone(),
                                        reactions,
                                    },
                                });
                                // A light nudge to the author — only on add, and not
                                // when reacting to your own message. In-app toast if
                                // they're connected; a quiet push if they're offline.
                                if added && author != principal.id {
                                    let _ = state.signal.send(Signal {
                                        room_id: room.clone(),
                                        target: Some(author.clone()),
                                        exclude: None,
                                        all_employees: false,
                                        frame: Outbound::ReactionNotice {
                                            room_id: room.clone(),
                                            message_id: id.clone(),
                                            emoji: emoji.clone(),
                                            from_name: principal.display_name.clone(),
                                        },
                                    });
                                    let state2 = state.clone();
                                    let from = principal.display_name.clone();
                                    tokio::spawn(async move {
                                        crate::send::push_reaction(
                                            &state2, &author, &from, &emoji, &room,
                                        )
                                        .await;
                                    });
                                }
                            }
                            _ => debug!(msg = %id, "react: message gone"),
                        }
                    }
                    Inbound::CallStart { room_id } => {
                        if !call_access(&state, &principal, &client_room, &room_id).await {
                            debug!(room = %room_id, "call start denied");
                            continue;
                        }
                        match state.calls.join(&room_id, &principal.id, &principal.display_name).await {
                            Ok((call_id, sdp)) => {
                                // An anonymous client starting a call rings EVERY
                                // employee cross-room (the per-room ring comes from
                                // join()), so anyone on the team can pick it up. The
                                // ring carries the room so the client-side shows the
                                // channel name and can switch into it on answer.
                                if !is_employee {
                                    let _ = state.signal.send(Signal {
                                        room_id: room_id.clone(),
                                        target: None,
                                        exclude: Some(principal.id.clone()),
                                        all_employees: true,
                                        frame: Outbound::CallRinging {
                                            call_id: call_id.clone(),
                                            room_id: room_id.clone(),
                                            from: principal.id.clone(),
                                            from_name: principal.display_name.clone(),
                                        },
                                    });
                                }
                                let frame = Outbound::CallOffer { call_id, sdp };
                                if let Ok(json) = serde_json::to_string(&frame) {
                                    if sender.send(Message::Text(json.into())).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            Err(e) => debug!(error = %e, "call start failed"),
                        }
                    }
                    Inbound::CallAnswer { call_id, sdp } => {
                        if let Err(e) = state.calls.answer(&call_id, &principal.id, sdp).await {
                            debug!(error = %e, "call answer failed");
                        }
                    }
                    Inbound::CallIce { call_id, candidate } => {
                        if let Err(e) = state.calls.ice(&call_id, &principal.id, candidate).await {
                            debug!(error = %e, "call ice failed");
                        }
                    }
                    Inbound::CallLeave { call_id } => {
                        state.calls.leave(&call_id, &principal.id).await;
                    }
                }
            }

            // Periodic latency probe: send a ping carrying the current time; the
            // pong (handled above) yields the round-trip.
            _ = ping_iv.tick() => {
                let payload = crate::now_millis().to_le_bytes().to_vec();
                if sender.send(Message::Ping(payload.into())).await.is_err() {
                    break;
                }
            }

            // Addressed signaling fan-out: deliver frames aimed at this principal,
            // or room-scoped frames for the room this socket is viewing.
            sig = sigrx.recv() => {
                match sig {
                    Ok(s) => {
                        let deliver = match &s.target {
                            Some(t) => *t == principal.id,
                            None => {
                                let in_room = s.room_id == active_room;
                                // Cross-room ring for employees viewing another room.
                                let cross_employee = s.all_employees && is_employee && !in_room;
                                (in_room || cross_employee)
                                    && s.exclude.as_deref() != Some(principal.id.as_str())
                            }
                        };
                        if deliver {
                            // A new DM with us → refresh accessible so its messages
                            // start bumping our unread badge right away.
                            if matches!(s.frame, Outbound::RoomsChanged) {
                                if let Ok(v) =
                                    db::list_rooms_for_user(&state.reads, &principal.id).await
                                {
                                    accessible = v.into_iter().map(|r| r.id).collect();
                                }
                            }
                            if let Ok(json) = serde_json::to_string(&s.frame) {
                                if sender.send(Message::Text(json.into())).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => break,
                }
            }

            // Client-message heads-up: employees get pinged about anonymous
            // rooms they aren't currently viewing (the room they're in already
            // shows the message live, so skip it to avoid a double-notify).
            note = notrx.recv() => {
                match note {
                    Ok(n) => {
                        if is_employee && n.room_id != active_room {
                            let frame = Outbound::ClientNotice { notice: n };
                            if let Ok(json) = serde_json::to_string(&frame) {
                                if sender.send(Message::Text(json.into())).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => break,
                }
            }

            // Presence fan-out: forward online/offline deltas to this client.
            pres = presrx.recv() => {
                match pres {
                    Ok(frame) => {
                        if let Ok(json) = serde_json::to_string(&frame) {
                            if sender.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => break,
                }
            }

            // Broadcast fan-out: forward only the active room (no cross-room leak).
            bcast = rx.recv() => {
                match bcast {
                    Ok(chat) if chat.room_id == active_room => {
                        let frame = Outbound::Message { message: Box::new(chat) };
                        if let Ok(json) = serde_json::to_string(&frame) {
                            if sender.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    // Another room employees can see → bump its unread badge (not
                    // for our own message; clients only see their own room). A
                    // `dm-` room is private, so only its members get the bump.
                    Ok(chat)
                        if is_employee
                            && chat.author_id != principal.id
                            && (!chat.room_id.starts_with("dm-")
                                || accessible.contains(&chat.room_id)) =>
                    {
                        let frame = Outbound::Unread { room_id: chat.room_id };
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
    // Read up to now for the room we were viewing, so a reconnect/reload doesn't
    // re-count messages we already saw while connected.
    let _ = db::mark_read(&state.db, &principal.id, &active_room, crate::now_millis()).await;
    state.presence.leave(&principal.id);
    info!("websocket closed");
}

/// Whether this socket's principal may start/join a call in `room_id`. Mirrors
/// the chat join rule: clients are pinned to their own room, employees any room.
async fn call_access(
    state: &AppState,
    principal: &Principal,
    client_room: &Option<String>,
    room_id: &str,
) -> bool {
    match client_room {
        Some(room) => room_id == room,
        None => db::can_access_room(&state.reads, &principal.kind, &principal.id, room_id)
            .await
            .unwrap_or(false),
    }
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
