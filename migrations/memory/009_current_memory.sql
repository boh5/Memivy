-- Memory is the working object; captures and old versions are recovery material.
-- The opener disables FK enforcement before BEGIN and checks all FKs before COMMIT.
DROP TRIGGER organization_attached;
DROP TRIGGER organization_hidden;
DROP TRIGGER organization_erased;
DROP TRIGGER capture_search_insert;
DROP TRIGGER version_search_insert;
DROP TRIGGER capture_search_erase;
DROP TRIGGER version_search_erase;

CREATE TABLE memories_new (
    id TEXT PRIMARY KEY NOT NULL,
    current_version_id TEXT REFERENCES memory_versions(id) DEFERRABLE INITIALLY DEFERRED,
    state TEXT NOT NULL DEFAULT 'active' CHECK(state IN ('active','trashed','undone','merged','purged')),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;
INSERT INTO memories_new SELECT * FROM memories;
DROP TABLE memories;
ALTER TABLE memories_new RENAME TO memories;
CREATE INDEX memories_recent ON memories(state, updated_at DESC, id);
CREATE TRIGGER memory_head BEFORE UPDATE OF current_version_id ON memories
WHEN NEW.current_version_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM memory_versions WHERE id=NEW.current_version_id AND memory_id=NEW.id
)
BEGIN SELECT RAISE(ABORT, 'current version belongs to another memory'); END;
CREATE TRIGGER remove_purged_memory_navigation AFTER UPDATE OF state ON memories
WHEN NEW.state='purged'
BEGIN
    DELETE FROM record_pins WHERE kind='memory' AND record_id=NEW.id;
    DELETE FROM collection_entries WHERE kind='memory' AND record_id=NEW.id;
END;
CREATE TRIGGER remove_purged_memory_feedback AFTER UPDATE OF state ON memories
WHEN NEW.state='purged'
BEGIN
    DELETE FROM collection_feedback WHERE receipt_id IN (SELECT request_id FROM receipts WHERE memory_id=NEW.id);
END;

CREATE TABLE receipt_changes_new (
    request_id TEXT NOT NULL REFERENCES receipts(request_id),
    memory_id TEXT NOT NULL REFERENCES memories(id),
    before_version TEXT REFERENCES memory_versions(id),
    after_version TEXT NOT NULL REFERENCES memory_versions(id),
    before_state TEXT NOT NULL CHECK(before_state IN ('active','undone','merged')),
    after_state TEXT NOT NULL CHECK(after_state IN ('active','undone','merged')),
    PRIMARY KEY(request_id,memory_id)
) STRICT;
INSERT INTO receipt_changes_new SELECT * FROM receipt_changes;
DROP TABLE receipt_changes;
ALTER TABLE receipt_changes_new RENAME TO receipt_changes;
CREATE INDEX receipt_changes_by_memory ON receipt_changes(memory_id,request_id);

-- Completed historical actions remain in receipts, not a second task registry.
DROP TABLE organization_jobs;
CREATE TABLE organization_jobs (
    memory_id TEXT PRIMARY KEY NOT NULL REFERENCES memories(id),
    input_version_id TEXT NOT NULL REFERENCES memory_versions(id),
    capture_id TEXT NOT NULL REFERENCES captures(id),
    attempt_id TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL CHECK(status IN ('pending','processing','done','failed','deferred','paused')),
    reason TEXT NOT NULL DEFAULT '',
    receipt_id TEXT,
    created_at INTEGER NOT NULL
) STRICT;
CREATE INDEX organization_pending ON organization_jobs(status,created_at,memory_id);
CREATE TRIGGER organization_hidden AFTER UPDATE OF state ON memories
WHEN NEW.state!='active'
BEGIN
    UPDATE organization_jobs SET status='paused' WHERE memory_id=NEW.id AND status IN ('pending','processing');
END;
CREATE TRIGGER organization_erased AFTER UPDATE OF state ON memories
WHEN NEW.state='purged'
BEGIN
    DELETE FROM organization_jobs WHERE memory_id=NEW.id;
END;
CREATE TABLE memory_keywords (
    version_id TEXT PRIMARY KEY NOT NULL REFERENCES memory_versions(id),
    terms TEXT NOT NULL
) STRICT;
PRAGMA user_version = 9;
