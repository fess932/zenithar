-- Packs can be public (shown on the owner's profile for anyone to add), like
-- saved items. And every sticker sent to a room carries its source pack's share
-- slug, so the recipient can tap it and add the whole pack — sharing a sticker
-- shares its pack (Telegram-style). NULL slug = a plain image, not a pack sticker.
ALTER TABLE saved_packs ADD COLUMN public INTEGER NOT NULL DEFAULT 0;
ALTER TABLE attachments ADD COLUMN pack_slug TEXT;
