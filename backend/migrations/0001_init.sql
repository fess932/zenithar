-- Zenithar schema — Phase 0.
-- Idempotent (IF NOT EXISTS) so startup can replay it safely until we add
-- a proper migration tracker.

CREATE TABLE IF NOT EXISTS users (
    id            TEXT PRIMARY KEY,           -- ULID
    login         TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,              -- argon2 (Phase 1)
    created_at    INTEGER NOT NULL            -- unix millis
);

CREATE TABLE IF NOT EXISTS clients (
    id         TEXT PRIMARY KEY,              -- ULID
    display    TEXT,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS rooms (
    id        TEXT PRIMARY KEY,               -- ULID; "common" reserved for the team room
    kind      TEXT NOT NULL CHECK (kind IN ('common', 'client')),
    client_id TEXT REFERENCES clients(id),
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS messages (
    id            TEXT PRIMARY KEY,           -- ULID (app-generated, sortable by time)
    room_id       TEXT NOT NULL REFERENCES rooms(id),
    author        TEXT NOT NULL,              -- Phase 0: free-form name; later user/client ref
    body          TEXT NOT NULL,
    client_msg_id TEXT,                       -- client UUID for idempotency
    created_at    INTEGER NOT NULL
);

-- Pagination of room history by ULID cursor.
CREATE INDEX IF NOT EXISTS idx_messages_room_id ON messages(room_id, id);
-- Idempotency: dedup retried/reconnected sends. NULLs are allowed and don't collide.
CREATE UNIQUE INDEX IF NOT EXISTS idx_messages_client_msg_id ON messages(client_msg_id);

-- Seed the common team room.
INSERT INTO rooms (id, kind, created_at)
VALUES ('common', 'common', 0)
ON CONFLICT(id) DO NOTHING;
