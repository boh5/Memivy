-- Phase 1 v2 experiment. Conversations never enter the capture FTS index.
CREATE TABLE prototype_topics (
    id TEXT PRIMARY KEY, title TEXT NOT NULL, draft TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
);
CREATE TABLE prototype_turns (
    id TEXT PRIMARY KEY, topic_id TEXT NOT NULL REFERENCES prototype_topics(id),
    question TEXT NOT NULL, answer TEXT, evidence TEXT NOT NULL DEFAULT '[]',
    status TEXT NOT NULL CHECK(status IN ('processing','complete','failed','cancelled','interrupted')),
    error TEXT, created_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX prototype_one_running_turn ON prototype_turns(topic_id) WHERE status='processing';
CREATE TABLE prototype_receipts (
    id TEXT PRIMARY KEY, capture_id TEXT NOT NULL UNIQUE REFERENCES captures(id),
    turn_id TEXT NOT NULL REFERENCES prototype_turns(id), title TEXT NOT NULL,
    undone INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL
);
CREATE TABLE prototype_withdrawn (capture_id TEXT PRIMARY KEY REFERENCES captures(id));
PRAGMA user_version = 2;
