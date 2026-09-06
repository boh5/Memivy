CREATE TABLE conversations (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    draft TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;
CREATE TABLE turns (
    id TEXT PRIMARY KEY NOT NULL,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    fingerprint BLOB NOT NULL
) STRICT;
CREATE TABLE messages (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    turn_id TEXT NOT NULL REFERENCES turns(id) ON DELETE CASCADE,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK(role IN ('user','assistant')),
    text TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('processing','complete','failed','cancelled','interrupted')),
    error_code TEXT,
    created_at INTEGER NOT NULL,
    UNIQUE(turn_id,role),
    CHECK(role='assistant' OR status='complete')
) STRICT;
CREATE UNIQUE INDEX one_running_answer ON messages(conversation_id) WHERE status='processing';
CREATE INDEX conversation_messages ON messages(conversation_id,seq);
CREATE TABLE message_citations (
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK(kind IN ('capture','version')),
    source_id TEXT NOT NULL,
    cited INTEGER NOT NULL DEFAULT 0 CHECK(cited IN (0,1)),
    PRIMARY KEY(message_id,kind,source_id)
) STRICT;
CREATE TABLE conclusion_intents (
    capture_id TEXT PRIMARY KEY NOT NULL REFERENCES captures(id),
    title TEXT NOT NULL,
    destination TEXT NOT NULL
) STRICT;
PRAGMA user_version = 2;
