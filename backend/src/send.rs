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
            preview: notice_preview(&chat.body, !chat.attachments.is_empty()),
            created_at: chat.created_at,
        });
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
