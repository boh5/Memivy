-- Local editing drafts are not durable memories and never enter FTS/MCP.
CREATE TABLE workspace_drafts (
    key TEXT PRIMARY KEY NOT NULL,
    payload TEXT NOT NULL
) STRICT;
CREATE INDEX memories_recent ON memories(state, updated_at DESC, id);
CREATE INDEX captures_recent ON captures(created_at DESC, id);
PRAGMA user_version = 3;
