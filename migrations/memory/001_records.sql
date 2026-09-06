-- Formal data only. Never apply these migrations to a Phase 1 database.
CREATE TABLE captures (
    id TEXT PRIMARY KEY NOT NULL,
    request_id TEXT NOT NULL UNIQUE,
    fingerprint BLOB NOT NULL,
    text TEXT,
    source TEXT,
    created_at INTEGER NOT NULL,
    CHECK ((text IS NULL) = (source IS NULL))
) STRICT;
CREATE TRIGGER captures_immutable BEFORE UPDATE ON captures
WHEN NEW.id IS NOT OLD.id OR NEW.request_id IS NOT OLD.request_id
 OR NEW.fingerprint IS NOT OLD.fingerprint OR NEW.created_at IS NOT OLD.created_at
 OR NEW.text IS NOT NULL OR NEW.source IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'capture content is immutable; explicit erasure only'); END;

CREATE TABLE memories (
    id TEXT PRIMARY KEY NOT NULL,
    current_version_id TEXT REFERENCES memory_versions(id) DEFERRABLE INITIALLY DEFERRED,
    state TEXT NOT NULL DEFAULT 'active' CHECK(state IN ('active','trashed','undone','purged')),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;
CREATE TABLE capture_state (
    capture_id TEXT PRIMARY KEY NOT NULL REFERENCES captures(id),
    availability TEXT NOT NULL DEFAULT 'active' CHECK(availability IN ('active','trashed','purged')),
    understanding TEXT NOT NULL DEFAULT 'pending' CHECK(understanding IN ('pending','attached','deferred')),
    trash_owner TEXT REFERENCES memories(id)
) STRICT;
CREATE TABLE memory_versions (
    id TEXT PRIMARY KEY NOT NULL,
    memory_id TEXT NOT NULL REFERENCES memories(id),
    parent_id TEXT REFERENCES memory_versions(id),
    title TEXT,
    body TEXT,
    actor TEXT NOT NULL CHECK(actor IN ('user','ai')),
    reason TEXT NOT NULL CHECK(reason IN ('create','append','edit','restore','undo')),
    created_at INTEGER NOT NULL,
    CHECK ((title IS NULL) = (body IS NULL))
) STRICT;
CREATE INDEX versions_by_memory ON memory_versions(memory_id, created_at);
CREATE TRIGGER versions_immutable BEFORE UPDATE ON memory_versions
WHEN NEW.id IS NOT OLD.id OR NEW.memory_id IS NOT OLD.memory_id
 OR NEW.parent_id IS NOT OLD.parent_id OR NEW.actor IS NOT OLD.actor
 OR NEW.reason IS NOT OLD.reason OR NEW.created_at IS NOT OLD.created_at
 OR NEW.title IS NOT NULL OR NEW.body IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'versions are immutable; explicit erasure only'); END;
CREATE TRIGGER version_parent BEFORE INSERT ON memory_versions
WHEN NEW.parent_id IS NOT NULL AND NOT EXISTS (
 SELECT 1 FROM memory_versions WHERE id=NEW.parent_id AND memory_id=NEW.memory_id
)
BEGIN SELECT RAISE(ABORT, 'version parent belongs to another memory'); END;
CREATE TRIGGER memory_head BEFORE UPDATE OF current_version_id ON memories
WHEN NEW.current_version_id IS NOT NULL AND NOT EXISTS (
 SELECT 1 FROM memory_versions WHERE id=NEW.current_version_id AND memory_id=NEW.id
)
BEGIN SELECT RAISE(ABORT, 'current version belongs to another memory'); END;
CREATE TABLE version_captures (
    version_id TEXT NOT NULL REFERENCES memory_versions(id),
    capture_id TEXT NOT NULL REFERENCES captures(id),
    PRIMARY KEY(version_id, capture_id)
) STRICT;
CREATE INDEX versions_by_capture ON version_captures(capture_id);
CREATE TABLE receipts (
    request_id TEXT PRIMARY KEY NOT NULL,
    fingerprint BLOB NOT NULL,
    action TEXT NOT NULL,
    capture_id TEXT REFERENCES captures(id),
    memory_id TEXT REFERENCES memories(id),
    before_version TEXT REFERENCES memory_versions(id),
    after_version TEXT REFERENCES memory_versions(id),
    status TEXT NOT NULL CHECK(status IN ('applied','needs_review','undone')),
    created_at INTEGER NOT NULL
) STRICT;
-- References keep their stable IDs even after a source's content is erased.
CREATE TABLE capture_citations (
    capture_id TEXT NOT NULL REFERENCES captures(id),
    kind TEXT NOT NULL CHECK(kind IN ('capture','version')),
    source_id TEXT NOT NULL,
    PRIMARY KEY(capture_id,kind,source_id)
) STRICT;
-- A correction changes two memories. Retain both effects for an atomic undo.
CREATE TABLE receipt_changes (
    request_id TEXT NOT NULL REFERENCES receipts(request_id),
    memory_id TEXT NOT NULL REFERENCES memories(id),
    before_version TEXT REFERENCES memory_versions(id),
    after_version TEXT NOT NULL REFERENCES memory_versions(id),
    before_state TEXT NOT NULL CHECK(before_state IN ('active','undone')),
    after_state TEXT NOT NULL CHECK(after_state IN ('active','undone')),
    PRIMARY KEY(request_id,memory_id)
) STRICT;
-- One derived trigram index. Availability is always checked against fact tables.
-- Historical versions remain addressable but ordinary search returns only heads.
CREATE VIRTUAL TABLE record_fts USING fts5(
    kind UNINDEXED, source_id UNINDEXED, title, body, origin, tokenize='trigram'
);
INSERT INTO record_fts(record_fts,rank) VALUES('secure-delete',1);
CREATE TRIGGER capture_search_insert AFTER INSERT ON captures WHEN NEW.text IS NOT NULL BEGIN
    INSERT INTO record_fts(kind,source_id,title,body,origin) VALUES('capture',NEW.id,'',NEW.text,NEW.source);
END;
CREATE TRIGGER version_search_insert AFTER INSERT ON memory_versions WHEN NEW.body IS NOT NULL BEGIN
    INSERT INTO record_fts(kind,source_id,title,body,origin) VALUES('version',NEW.id,NEW.title,NEW.body,'');
END;
CREATE TRIGGER capture_search_erase AFTER UPDATE ON captures WHEN NEW.text IS NULL BEGIN
    DELETE FROM record_fts WHERE kind='capture' AND source_id=NEW.id;
END;
CREATE TRIGGER version_search_erase AFTER UPDATE ON memory_versions WHEN NEW.body IS NULL BEGIN
    DELETE FROM record_fts WHERE kind='version' AND source_id=NEW.id;
END;
PRAGMA user_version = 1;
