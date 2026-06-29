-- Device push tokens (FCM) for offline notifications. One row per device token;
-- the token is the natural key, so re-registering the same device is idempotent
-- and a device that switches accounts just re-points to the new principal.
CREATE TABLE IF NOT EXISTS push_tokens (
    token        TEXT PRIMARY KEY,
    principal_id TEXT NOT NULL REFERENCES principals(id),
    platform     TEXT NOT NULL DEFAULT 'android',
    created_at   INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_push_tokens_principal ON push_tokens(principal_id);
