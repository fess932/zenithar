-- Zenithar schema — Phase 1 (passwordless link auth).
-- Idempotent (IF NOT EXISTS). Pre-release we keep ONE migration file and
-- recreate the DB (`make db-reset`) instead of adding incremental migrations.

-- Unified identities: employees (kind='user') and anonymous clients
-- (kind='client'). Everyone authenticates by a link-token, not a password.
CREATE TABLE IF NOT EXISTS principals (
    id           TEXT PRIMARY KEY,           -- ULID
    kind         TEXT NOT NULL CHECK (kind IN ('user', 'client', 'bot')),
    display_name TEXT NOT NULL,              -- random at creation; users rename themselves
    is_admin     INTEGER NOT NULL DEFAULT 0, -- 0/1, only meaningful for 'user'
    created_at   INTEGER NOT NULL            -- unix millis
);

-- Durable link-tokens ("login by link"). One active token per principal;
-- rotation revokes the previous one (same principal_id → same dialog/identity).
CREATE TABLE IF NOT EXISTS tokens (
    id           TEXT PRIMARY KEY,           -- ULID
    token_hash   TEXT NOT NULL UNIQUE,       -- SHA-256 of the token; plaintext never stored
    principal_id TEXT NOT NULL REFERENCES principals(id),
    created_at   INTEGER NOT NULL,
    revoked_at   INTEGER,                    -- NULL = active
    rotated_from TEXT REFERENCES tokens(id)  -- audit trail of reissues
);
CREATE INDEX IF NOT EXISTS idx_tokens_principal ON tokens(principal_id);

-- API tokens for integrations (Phase 6). A `bot` principal authenticates over
-- REST with `Authorization: Bearer zk_…`; like link-tokens we store only the
-- SHA-256 hash and show the plaintext once at issue time. `last_used_at` is a
-- best-effort touch for the admin UI; rotation revokes the previous token.
CREATE TABLE IF NOT EXISTS api_tokens (
    id           TEXT PRIMARY KEY,           -- ULID
    token_hash   TEXT NOT NULL UNIQUE,       -- SHA-256 of the token; plaintext never stored
    principal_id TEXT NOT NULL REFERENCES principals(id),
    name         TEXT NOT NULL,              -- human label, e.g. "CRM"
    created_at   INTEGER NOT NULL,
    last_used_at INTEGER,                    -- best-effort, updated on use
    revoked_at   INTEGER                     -- NULL = active
);
CREATE INDEX IF NOT EXISTS idx_api_tokens_principal ON api_tokens(principal_id);

-- Cookie sessions: a link-token is exchanged for an httpOnly cookie on first visit.
CREATE TABLE IF NOT EXISTS sessions (
    token_hash   TEXT PRIMARY KEY,           -- SHA-256 of the session cookie value
    principal_id TEXT NOT NULL REFERENCES principals(id),
    created_at   INTEGER NOT NULL,
    expires_at   INTEGER NOT NULL,
    last_seen    INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS rooms (
    id         TEXT PRIMARY KEY,             -- ULID; "common" reserved for the team room
    kind       TEXT NOT NULL CHECK (kind IN ('common', 'client')),
    client_id  TEXT REFERENCES principals(id),
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS messages (
    id            TEXT PRIMARY KEY,          -- ULID (app-generated, sortable by time)
    room_id       TEXT NOT NULL REFERENCES rooms(id),
    author_id     TEXT NOT NULL REFERENCES principals(id),
    author_name   TEXT NOT NULL,            -- denormalized display name at send time
    body          TEXT NOT NULL,
    reply_to      TEXT REFERENCES messages(id), -- replied-to message (Telegram-style quote)
    client_msg_id TEXT,                      -- client UUID for idempotency
    created_at    INTEGER NOT NULL
);

-- Uploaded files (images get a thumbnail + dimensions). The id doubles as the
-- storage key; bytes live on disk (or S3 later) via the Storage trait, not here.
-- A message can carry several attachments (up to 5); `message_id` is NULL between
-- upload and the message being sent.
CREATE TABLE IF NOT EXISTS attachments (
    id           TEXT PRIMARY KEY,          -- ULID; also the disk key
    room_id      TEXT NOT NULL REFERENCES rooms(id),
    uploader_id  TEXT NOT NULL REFERENCES principals(id),
    message_id   TEXT REFERENCES messages(id), -- set when the message is sent
    filename     TEXT NOT NULL,             -- original name (display only)
    content_type TEXT NOT NULL,             -- sniffed/validated mime
    size         INTEGER NOT NULL,          -- bytes
    width        INTEGER,                   -- images only
    height       INTEGER,
    has_thumb    INTEGER NOT NULL DEFAULT 0,
    created_at   INTEGER NOT NULL
);

-- Voice calls (Phase 4). One call per room at a time; the server is the WebRTC
-- peer in the middle. `recording_id` is filled by Phase 5 (server-side recording).
CREATE TABLE IF NOT EXISTS calls (
    id           TEXT PRIMARY KEY,          -- ULID = call_id
    room_id      TEXT NOT NULL REFERENCES rooms(id),
    started_by   TEXT NOT NULL REFERENCES principals(id),
    started_at   INTEGER NOT NULL,          -- unix millis
    ended_at     INTEGER,                   -- NULL while live
    recording_id TEXT                       -- Phase 5: blob/attachment key
);
CREATE INDEX IF NOT EXISTS idx_calls_room ON calls(room_id);

CREATE INDEX IF NOT EXISTS idx_messages_room_id ON messages(room_id, id);
CREATE INDEX IF NOT EXISTS idx_messages_reply_to ON messages(reply_to);
CREATE UNIQUE INDEX IF NOT EXISTS idx_messages_client_msg_id ON messages(client_msg_id);
CREATE INDEX IF NOT EXISTS idx_attachments_message ON attachments(message_id);

-- Seed the common team room.
INSERT INTO rooms (id, kind, created_at)
VALUES ('common', 'common', 0)
ON CONFLICT(id) DO NOTHING;

-- Last-seen per principal (presence "ping"), persisted so the connections list
-- survives a server restart instead of showing a dash.
CREATE TABLE IF NOT EXISTS last_seen (
    principal_id TEXT PRIMARY KEY,
    ts           INTEGER NOT NULL   -- unix millis of last activity
);
