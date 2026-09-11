-- Vectors remain disposable. Allow the dimension verified for each active model.
DROP TRIGGER embedding_erased;
CREATE TABLE embedding_chunks_new (
    memory_id TEXT NOT NULL REFERENCES memories(id),
    version_id TEXT NOT NULL REFERENCES memory_versions(id),
    ordinal INTEGER NOT NULL,
    start_char INTEGER NOT NULL CHECK(start_char>=0),
    end_char INTEGER NOT NULL CHECK(end_char>start_char),
    input_hash TEXT NOT NULL,
    vector BLOB NOT NULL CHECK(typeof(vector)='blob' AND length(vector)>=4 AND length(vector)<=65536 AND length(vector)%4=0),
    PRIMARY KEY(memory_id,ordinal)
) STRICT;
INSERT INTO embedding_chunks_new SELECT * FROM embedding_chunks;
DROP TABLE embedding_chunks;
ALTER TABLE embedding_chunks_new RENAME TO embedding_chunks;
CREATE INDEX embedding_input_hash ON embedding_chunks(memory_id,input_hash);
CREATE TRIGGER embedding_erased AFTER UPDATE OF body ON memory_versions WHEN NEW.body IS NULL
BEGIN
    DELETE FROM embedding_chunks WHERE version_id=NEW.id;
    DELETE FROM embedding_records WHERE version_id=NEW.id;
END;
PRAGMA user_version=14;
