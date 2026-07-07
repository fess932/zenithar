-- Sticker/emoji packs inside "сохранёнки". A pack groups saved_items (each still
-- its own Storage blob) under a name, shown as a separate sub-list. Packs are
-- shared by an unguessable `share_slug` (Telegram addstickers-style): anyone with
-- the link can copy the whole pack into their own collection.
CREATE TABLE saved_packs (
    id            TEXT PRIMARY KEY,
    owner_id      TEXT NOT NULL,
    name          TEXT NOT NULL,
    -- What the pack holds, so the UI files it under the right sub-list:
    -- 'sticker' | 'gif' | 'saved' (default). Room for 'emoji' etc. later.
    kind          TEXT NOT NULL DEFAULT 'saved',
    cover_item_id TEXT,               -- one member item, for the pack tile
    share_slug    TEXT NOT NULL UNIQUE,
    created_at    INTEGER NOT NULL
);
CREATE INDEX idx_saved_packs_owner ON saved_packs (owner_id, created_at DESC);

-- NULL pack_id = a loose saved image (the existing behaviour); set = pack member.
ALTER TABLE saved_items ADD COLUMN pack_id TEXT;
CREATE INDEX idx_saved_items_pack ON saved_items (pack_id);
