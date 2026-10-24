-- Imported sources have unknown authorship. Schema 3 keeps older clients from
-- interpreting the new origin kind without this provenance policy.
CREATE TABLE import_batches (
    request_id TEXT PRIMARY KEY NOT NULL REFERENCES receipts(request_id),
    result TEXT NOT NULL CHECK(json_valid(result))
) STRICT;

-- The digest narrows exact duplicate checks; the original text is compared too.
-- This is a deduplication index, not a second memory search index.
CREATE TABLE import_sources (
    capture_id TEXT PRIMARY KEY NOT NULL REFERENCES captures(id),
    content_hash BLOB NOT NULL CHECK(length(content_hash)=32)
) STRICT;
CREATE INDEX import_sources_by_content ON import_sources(content_hash);
