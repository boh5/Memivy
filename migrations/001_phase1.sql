-- Isolated prototype schema, not the production memory/version data model.
CREATE TABLE captures (
    id TEXT NOT NULL UNIQUE,
    request_id TEXT NOT NULL UNIQUE,
    text TEXT NOT NULL,
    source_app TEXT NOT NULL,
    project TEXT,
    session_uri TEXT,
    created_at INTEGER NOT NULL,
    ai_state TEXT NOT NULL DEFAULT 'pending' CHECK (ai_state = 'pending')
);
CREATE VIRTUAL TABLE captures_fts USING fts5(
    text, source_app, content='captures', content_rowid='rowid', tokenize='trigram'
);
CREATE TRIGGER capture_index AFTER INSERT ON captures BEGIN
    INSERT INTO captures_fts(rowid, text, source_app) VALUES(new.rowid, new.text, new.source_app);
END;
CREATE TRIGGER capture_immutable BEFORE UPDATE ON captures BEGIN
    SELECT RAISE(ABORT, 'phase1 captures are immutable');
END;
PRAGMA user_version = 1;
