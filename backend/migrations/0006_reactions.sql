-- Emoji reactions on messages (Telegram-style). One row per (message, principal,
-- emoji): a principal may add several distinct emoji to a message, and toggling
-- the same emoji again removes its row. Counts/highlighting are derived from the
-- set of rows per message.
CREATE TABLE IF NOT EXISTS reactions (
    message_id   TEXT NOT NULL REFERENCES messages(id),
    principal_id TEXT NOT NULL REFERENCES principals(id),
    emoji        TEXT NOT NULL,
    created_at   INTEGER NOT NULL,
    PRIMARY KEY (message_id, principal_id, emoji)
);

CREATE INDEX IF NOT EXISTS idx_reactions_message ON reactions(message_id);
