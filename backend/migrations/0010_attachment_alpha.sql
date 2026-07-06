-- Transparency flag for image attachments. The UI renders images with an alpha
-- channel (transparent PNG/WebP "stickers") frameless — no border or surface
-- behind them — so the transparency reads the way the sender intended. Existing
-- rows default to 0 (opaque, framed as before).
ALTER TABLE attachments ADD COLUMN has_alpha INTEGER NOT NULL DEFAULT 0;
