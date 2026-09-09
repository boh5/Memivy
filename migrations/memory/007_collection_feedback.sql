-- Recommendations and dismissal belong to one immutable organization receipt.
CREATE TABLE collection_feedback (
    receipt_id TEXT PRIMARY KEY NOT NULL REFERENCES receipts(request_id) ON DELETE CASCADE,
    suggestions TEXT NOT NULL DEFAULT '[]',
    dismissed INTEGER NOT NULL DEFAULT 0 CHECK(dismissed IN (0,1))
) STRICT;
PRAGMA user_version = 7;
CREATE TRIGGER remove_purged_memory_feedback AFTER UPDATE OF state ON memories
WHEN NEW.state='purged'
BEGIN
    DELETE FROM collection_feedback WHERE receipt_id IN (SELECT request_id FROM receipts WHERE memory_id=NEW.id);
END;
CREATE TRIGGER remove_purged_capture_feedback AFTER UPDATE OF availability ON capture_state
WHEN NEW.availability='purged'
BEGIN
    DELETE FROM collection_feedback WHERE receipt_id IN (SELECT request_id FROM receipts WHERE capture_id=NEW.capture_id);
END;
