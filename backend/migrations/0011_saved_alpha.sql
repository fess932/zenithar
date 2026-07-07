-- Saved items lost their transparency flag on the way in and out: re-sending a
-- transparent image from сохранёнки flattened it to a black JPEG thumb inside a
-- framed bubble. Carry `has_alpha` through the saved pipeline like attachments do.
ALTER TABLE saved_items ADD COLUMN has_alpha INTEGER NOT NULL DEFAULT 0;
