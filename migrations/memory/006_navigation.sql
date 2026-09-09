-- Personal navigation metadata does not mutate memory content or its versions.
CREATE TABLE record_pins (
    kind TEXT NOT NULL CHECK(kind IN ('memory','capture')),
    record_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY(kind,record_id)
) STRICT;
CREATE TABLE collections (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    archived INTEGER NOT NULL DEFAULT 0 CHECK(archived IN (0,1)),
    revision INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL
) STRICT;
CREATE UNIQUE INDEX collection_names ON collections(name COLLATE NOCASE) WHERE archived=0;
CREATE TABLE collection_entries (
    collection_id TEXT NOT NULL REFERENCES collections(id),
    kind TEXT NOT NULL CHECK(kind IN ('memory','capture')),
    record_id TEXT NOT NULL,
    PRIMARY KEY(collection_id,kind,record_id)
) STRICT;
CREATE INDEX collections_by_record ON collection_entries(kind,record_id,collection_id);
CREATE TABLE conversation_collections (
    conversation_id TEXT PRIMARY KEY NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    collection_id TEXT NOT NULL REFERENCES collections(id)
) STRICT;
CREATE TRIGGER remove_purged_memory_navigation AFTER UPDATE OF state ON memories
WHEN NEW.state='purged'
BEGIN
    DELETE FROM record_pins WHERE kind='memory' AND record_id=NEW.id;
    DELETE FROM collection_entries WHERE kind='memory' AND record_id=NEW.id;
END;
CREATE TRIGGER remove_purged_capture_navigation AFTER UPDATE OF availability ON capture_state
WHEN NEW.availability='purged'
BEGIN
    DELETE FROM record_pins WHERE kind='capture' AND record_id=NEW.capture_id;
    DELETE FROM collection_entries WHERE kind='capture' AND record_id=NEW.capture_id;
END;
PRAGMA user_version = 6;
