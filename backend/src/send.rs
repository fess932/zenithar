//! The one place a chat message gets delivered: realtime fan-out, optional
//! employee heads-up, then durable (batched) write. Both the WebSocket handler
//! and the REST API funnel through [`deliver`] so the two paths can't drift.

use crate::models::{ChatMessage, ClientNotice};
use crate::state::AppState;
use crate::writer::WriteCmd;

/// Fan out `chat` to subscribers, optionally ping employees (only when an
/// anonymous client wrote), and enqueue the durable write. Returns `Err(())` if
/// the writer is gone (the caller should treat that as fatal for its loop).
pub async fn deliver(
    state: &AppState,
    chat: ChatMessage,
    notify_employees: bool,
) -> Result<(), ()> {
    // Realtime first: everyone subscribed to the room gets it immediately.
    let _ = state.broadcast.send(chat.clone());

    // Cross-room heads-up for employees about anonymous client rooms.
    if notify_employees {
        let _ = state.notify.send(ClientNotice {
            room_id: chat.room_id.clone(),
            from_name: chat.author_name.clone(),
            preview: chat_preview(&chat),
            created_at: chat.created_at,
        });
    }

    // Offline push: members of this room with no live WebSocket get an FCM nudge.
    // Fire-and-forget on its own task so it never delays delivery or the write.
    if state.push.is_some() {
        let state = state.clone();
        let chat = chat.clone();
        tokio::spawn(async move { push_offline(&state, &chat).await });
    }

    // Durability second: the batching writer commits it.
    state
        .writes
        .send(WriteCmd {
            msg: chat,
            ack: None,
        })
        .await
        .map_err(|_| ())
}

/// Push `chat` to every room member who is currently offline (no live socket),
/// excluding the author. Best-effort: logs and moves on; a dead token is pruned.
async fn push_offline(state: &AppState, chat: &ChatMessage) {
    let Some(fcm) = &state.push else { return };

    let audience = match crate::db::room_audience(&state.reads, &chat.room_id).await {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!(error = %e, "push: room_audience failed");
            return;
        }
    };
    let targets: Vec<String> = audience
        .into_iter()
        .filter(|id| id != &chat.author_id && !state.presence.is_online(id))
        .collect();
    if targets.is_empty() {
        return;
    }

    let tokens = match crate::db::tokens_for_principals(&state.reads, &targets).await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(error = %e, "push: token lookup failed");
            return;
        }
    };

    let title = if chat.author_name.is_empty() {
        "Новое сообщение".to_string()
    } else {
        chat.author_name.clone()
    };
    let body = chat_preview(chat);

    for (token, _pid) in tokens {
        match fcm.send(&token, &title, &body, &chat.room_id).await {
            Ok(true) => {}
            // Token is dead — drop it so we stop trying.
            Ok(false) => {
                let _ = crate::db::delete_push_token(&state.db, &token).await;
            }
            Err(e) => tracing::warn!(error = %e, "push: FCM send failed"),
        }
    }
}

/// Push a light reaction nudge to the message `author` if they're offline — a
/// quiet "❤️" rather than a message. Online authors get the in-app toast instead.
pub async fn push_reaction(
    state: &AppState,
    author_id: &str,
    from_name: &str,
    emoji: &str,
    room_id: &str,
) {
    let Some(fcm) = &state.push else { return };
    if state.presence.is_online(author_id) {
        return; // online → the in-app reaction toast covers it
    }
    let ids = [author_id.to_string()];
    let tokens = match crate::db::tokens_for_principals(&state.reads, &ids).await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(error = %e, "react push: token lookup failed");
            return;
        }
    };
    let title = if from_name.is_empty() {
        emoji
    } else {
        from_name
    };
    for (token, _pid) in tokens {
        match fcm.send(&token, title, emoji, room_id).await {
            Ok(true) => {}
            Ok(false) => {
                let _ = crate::db::delete_push_token(&state.db, &token).await;
            }
            Err(e) => tracing::warn!(error = %e, "react push: FCM send failed"),
        }
    }
}

/// Notification preview for a whole message: "Стикер" for a sticker, else the
/// body (or an attachment marker).
fn chat_preview(chat: &ChatMessage) -> String {
    if chat.sticker.is_some() {
        return "Стикер".to_string();
    }
    notice_preview(&chat.body, !chat.attachments.is_empty())
}

/// A short, single-line preview for a notification: the trimmed body (capped),
/// or a paperclip marker when the message is attachment-only.
pub fn notice_preview(body: &str, has_attachment: bool) -> String {
    const MAX: usize = 80;
    let body = body.trim();
    if body.is_empty() {
        return if has_attachment {
            "📎".to_string()
        } else {
            String::new()
        };
    }
    if body.chars().count() > MAX {
        format!("{}…", body.chars().take(MAX).collect::<String>())
    } else {
        body.to_string()
    }
}
