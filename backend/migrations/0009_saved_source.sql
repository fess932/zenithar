-- Minimal dedup for "сохранёнки": remember which message attachment a saved item
-- was copied from, so re-saving the SAME attachment returns the existing copy
-- instead of making another. NULL for directly-uploaded items (no source).
ALTER TABLE saved_items ADD COLUMN source_id TEXT;

CREATE INDEX IF NOT EXISTS idx_saved_source ON saved_items(principal_id, source_id);
