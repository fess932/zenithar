-- Sticker messages (Telegram-style). A sticker message carries a short sticker
-- id here (e.g. "heart") instead of a body; the client renders the matching
-- bundled animation. NULL for ordinary messages. Reference model — no per-send
-- blob (unlike attachments); a later "packs" feature reuses the same id field.
ALTER TABLE messages ADD COLUMN sticker TEXT;
