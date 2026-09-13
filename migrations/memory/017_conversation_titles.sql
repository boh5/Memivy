-- Titles are generated once, on demand after a successful discussion. Existing
-- conversations keep their current title until a future successful discussion.
ALTER TABLE conversations ADD COLUMN title_generated INTEGER NOT NULL DEFAULT 0 CHECK(title_generated IN (0,1));

PRAGMA user_version = 17;
