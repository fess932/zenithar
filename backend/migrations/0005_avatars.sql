-- Per-principal avatar. NULL = no choice yet → the client renders a default
-- emoji derived from the id. A custom value is either an emoji grapheme (shown
-- as-is) or "photo:<millis>" — the bytes live in Storage under `av_<id>` and are
-- served by GET /api/avatars/<id>; the millis suffix busts the image cache.
ALTER TABLE principals ADD COLUMN avatar TEXT;
