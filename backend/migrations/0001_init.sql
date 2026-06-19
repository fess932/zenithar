-- Zenithar schema — Phase 1 (passwordless link auth).
-- Idempotent (IF NOT EXISTS). Pre-release we keep ONE migration file and
-- recreate the DB (`make db-reset`) instead of adding incremental migrations.

-- Unified identities: employees (kind='user') and anonymous clients
-- (kind='client'). Everyone authenticates by a link-token, not a password.
CREATE TABLE IF NOT EXISTS principals (
    id           TEXT PRIMARY KEY,           -- ULID
    kind         TEXT NOT NULL CHECK (kind IN ('user', 'client')),
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
    client_msg_id TEXT,                      -- client UUID for idempotency
    created_at    INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_messages_room_id ON messages(room_id, id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_messages_client_msg_id ON messages(client_msg_id);

-- Seed the common team room.
INSERT INTO rooms (id, kind, created_at)
VALUES ('common', 'common', 0)
ON CONFLICT(id) DO NOTHING;
