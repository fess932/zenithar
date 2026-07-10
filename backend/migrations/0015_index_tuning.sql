-- Index tuning, driven by EXPLAIN QUERY PLAN on the hot read paths.
--
-- 1. messages(room_id, created_at, id, author_id)
--    The existing idx_messages_room_id is (room_id, id) — great for backwards
--    pagination (`id < ?`, ULIDs sort by time), useless for anything keyed on
--    created_at. Two hot queries were paying for that:
--      * unread_counts() filtered `created_at > last_read_at` with no usable
--        index, so it walked EVERY message in the room, per room, per connect.
--        Now a covering range seek: cost is the unread count, not the room size.
--      * list_rooms_for_user()'s per-room "last message" subqueries sorted the
--        whole room into a TEMP B-TREE just to take LIMIT 1.
--    `id` is the created_at tiebreak (ORDER BY created_at DESC, id DESC);
--    `author_id` rides along only so the unread COUNT stays index-only.
--    idx_messages_room_id stays — it's still what serves `id < ?` pagination.
CREATE INDEX IF NOT EXISTS idx_messages_room_created
    ON messages (room_id, created_at, id, author_id);

-- 2. room_reads(room_id, last_read_at)
--    others_read_at() filters by room_id, which is the SECOND column of the
--    (principal_id, room_id) primary key — so it full-scanned the table on
--    every message send (read receipts).
CREATE INDEX IF NOT EXISTS idx_room_reads_room
    ON room_reads (room_id, last_read_at);

-- 3. rooms(client_id)
--    ensure_client_room() looked a room up by client_id with a full scan. The
--    table is tiny today, but it grows with every client and this also gives
--    the client_id foreign key a child index.
CREATE INDEX IF NOT EXISTS idx_rooms_client
    ON rooms (client_id);

-- 4. Drop idx_reactions_message — pure write amplification.
--    reactions' primary key is (message_id, principal_id, emoji), and its
--    implicit index already has message_id as the leftmost column. That serves
--    both the by-message lookups and the foreign-key child scan on message
--    delete; the planner picks it over this index anyway. Verified: dropping it
--    changes no query plan.
DROP INDEX IF EXISTS idx_reactions_message;
