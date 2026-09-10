CREATE TABLE message_evidence_spans (
    message_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    source_id TEXT NOT NULL,
    start_char INTEGER NOT NULL CHECK(start_char>=0),
    length_chars INTEGER NOT NULL CHECK(length_chars>0),
    PRIMARY KEY(message_id,kind,source_id,start_char),
    FOREIGN KEY(message_id,kind,source_id)
        REFERENCES message_citations(message_id,kind,source_id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version = 11;
