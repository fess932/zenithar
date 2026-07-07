-- Mark an attachment / saved item as a sticker so the client renders it bare
-- (frameless, autoplaying) instead of a framed photo or a video player with
-- controls. Pack items are stickers; this rides along when one is sent to a room.
ALTER TABLE attachments ADD COLUMN is_sticker INTEGER NOT NULL DEFAULT 0;
ALTER TABLE saved_items ADD COLUMN is_sticker INTEGER NOT NULL DEFAULT 0;
