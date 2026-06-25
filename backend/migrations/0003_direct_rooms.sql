-- no-transaction
-- Direct (1:1) messages between employees. Reuses the rooms/messages machinery,
-- but a `direct` room is private to its two members (unlike common/client rooms,
-- which every employee sees).
--
-- Runs WITHOUT a transaction (the `-- no-transaction` directive above) so we can
-- toggle `PRAGMA foreign_keys` — a no-op inside a transaction. That's required to
-- widen rooms.kind: SQLite can't ALTER a CHECK constraint, so we rebuild the
-- table, and with foreign_keys OFF the referencing messages/calls/etc. survive
-- the DROP (their ids all reappear in the renamed table). The CHECK is dropped
-- entirely so future room kinds don't need another rebuild.

PRAGMA foreign_keys = OFF;

CREATE TABLE rooms_new (
    id         TEXT PRIMARY KEY,
    kind       TEXT NOT NULL,                -- 'common' | 'client' | 'direct'
    client_id  TEXT REFERENCES principals(id),
    created_at INTEGER NOT NULL
);
INSERT INTO rooms_new (id, kind, client_id, created_at)
    SELECT id, kind, client_id, created_at FROM rooms;
DROP TABLE rooms;
ALTER TABLE rooms_new RENAME TO rooms;

PRAGMA foreign_keys = ON;

-- Membership for private rooms (direct now; groups later). Common/client rooms
-- don't use this — their visibility is by role.
CREATE TABLE IF NOT EXISTS room_members (
    room_id      TEXT NOT NULL REFERENCES rooms(id),
    principal_id TEXT NOT NULL REFERENCES principals(id),
    PRIMARY KEY (room_id, principal_id)
);
CREATE INDEX IF NOT EXISTS idx_room_members_principal ON room_members(principal_id);
