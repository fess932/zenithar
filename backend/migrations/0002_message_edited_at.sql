-- Message editing: stamp when an author edits the body (NULL = never edited).
ALTER TABLE messages ADD COLUMN edited_at INTEGER;
