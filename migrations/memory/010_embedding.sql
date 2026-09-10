-- Disposable vectors, one active encoding configuration. Facts stay in Memory.
CREATE TABLE embedding_index_meta (
    id INTEGER PRIMARY KEY CHECK(id=1),
    fingerprint TEXT NOT NULL,
    revision TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('building','ready')),
    upper_rowid INTEGER NOT NULL,
    cursor INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE embedding_records (
    memory_id TEXT PRIMARY KEY REFERENCES memories(id),
    version_id TEXT NOT NULL REFERENCES memory_versions(id),
    input_hash TEXT NOT NULL,
    error TEXT
) STRICT;
CREATE TABLE embedding_chunks (
    memory_id TEXT NOT NULL REFERENCES memories(id),
    version_id TEXT NOT NULL REFERENCES memory_versions(id),
    ordinal INTEGER NOT NULL,
    start_char INTEGER NOT NULL CHECK(start_char>=0),
    end_char INTEGER NOT NULL CHECK(end_char>start_char),
    input_hash TEXT NOT NULL,
    vector BLOB NOT NULL CHECK(typeof(vector)='blob' AND length(vector)=4096),
    PRIMARY KEY(memory_id,ordinal)
) STRICT;
CREATE INDEX embedding_input_hash ON embedding_chunks(memory_id,input_hash);
CREATE TRIGGER embedding_erased AFTER UPDATE OF body ON memory_versions WHEN NEW.body IS NULL
BEGIN
    DELETE FROM embedding_chunks WHERE version_id=NEW.id;
    DELETE FROM embedding_records WHERE version_id=NEW.id;
END;
PRAGMA user_version=10;
