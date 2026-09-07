-- Processing is independent of disposition: undo must never enqueue AI again.
CREATE TABLE organization_jobs (
 capture_id TEXT PRIMARY KEY REFERENCES captures(id),
 attempt_id TEXT NOT NULL UNIQUE,
 status TEXT NOT NULL CHECK(status IN ('pending','processing','done','failed','deferred','paused')),
 reason TEXT NOT NULL DEFAULT '',
 receipt_id TEXT,
 created_at INTEGER NOT NULL
) STRICT;
CREATE TABLE capture_keywords (
 capture_id TEXT PRIMARY KEY REFERENCES captures(id),
 terms TEXT NOT NULL
) STRICT;
CREATE TRIGGER organization_attached AFTER UPDATE OF understanding ON capture_state
WHEN NEW.understanding='attached' BEGIN
 UPDATE organization_jobs SET status='paused' WHERE capture_id=NEW.capture_id AND status IN ('pending','processing');
END;
CREATE TRIGGER organization_hidden AFTER UPDATE OF availability ON capture_state
WHEN NEW.availability!='active' BEGIN
 UPDATE organization_jobs SET status='paused' WHERE capture_id=NEW.capture_id AND status IN ('pending','processing');
END;
CREATE TRIGGER organization_erased AFTER UPDATE ON captures WHEN NEW.text IS NULL BEGIN
 DELETE FROM organization_jobs WHERE capture_id=NEW.id;
 DELETE FROM capture_keywords WHERE capture_id=NEW.id;
END;
ALTER TABLE messages ADD COLUMN answer TEXT;
ALTER TABLE message_citations ADD COLUMN excerpt_start INTEGER NOT NULL DEFAULT 0;
ALTER TABLE message_citations ADD COLUMN excerpt_length INTEGER NOT NULL DEFAULT 1800;
PRAGMA user_version = 4;
