-- Retain the exact user-reviewed integration even when its target changed.
ALTER TABLE conclusion_intents ADD COLUMN merged_body TEXT;
PRAGMA user_version = 5;
