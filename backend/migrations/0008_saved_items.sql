-- "Сохранёнки" (VK-style saved images): a private per-user collection. Each row
-- is the user's OWN copy of an image (its own Storage blob keyed by `id`), so it
-- survives deletion of the message it was saved from. `public` lets the owner
-- expose an item on their profile. A later "packs" / wider media feature can grow
-- this table; for now it's images saved from messages or uploaded directly.
CREATE TABLE IF NOT EXISTS saved_items (
    id           TEXT PRIMARY KEY,          -- ULID; also the disk/blob key
    principal_id TEXT NOT NULL REFERENCES principals(id),
    filename     TEXT NOT NULL,
    content_type TEXT NOT NULL,
    size         INTEGER NOT NULL,
    width        INTEGER,
    height       INTEGER,
    has_thumb    INTEGER NOT NULL DEFAULT 0,
    public       INTEGER NOT NULL DEFAULT 0,
    created_at   INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_saved_principal ON saved_items(principal_id, created_at);
