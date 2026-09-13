-- A turn is one logical user input. Attempts replace execution, never its messages.
ALTER TABLE turns ADD COLUMN active_attempt TEXT;
ALTER TABLE turns ADD COLUMN protocol_messages TEXT NOT NULL DEFAULT '[]';
ALTER TABLE turns ADD COLUMN focused_memory_ids TEXT NOT NULL DEFAULT '[]';
ALTER TABLE turns ADD COLUMN input_origin TEXT;
ALTER TABLE turns ADD COLUMN checkpoint_text_chars INTEGER NOT NULL DEFAULT 0;
ALTER TABLE turns ADD COLUMN follow_ups TEXT NOT NULL DEFAULT '[]';
ALTER TABLE turns ADD COLUMN progress TEXT;
ALTER TABLE turns ADD COLUMN record_only INTEGER NOT NULL DEFAULT 0 CHECK(record_only IN (0,1));
ALTER TABLE turns ADD COLUMN maintenance_paused INTEGER NOT NULL DEFAULT 0 CHECK(maintenance_paused IN (0,1));
ALTER TABLE conversations ADD COLUMN summary TEXT NOT NULL DEFAULT '';
ALTER TABLE conversations ADD COLUMN summary_through_seq INTEGER NOT NULL DEFAULT 0;
ALTER TABLE conversations ADD COLUMN memory_paused INTEGER NOT NULL DEFAULT 0 CHECK(memory_paused IN (0,1));

-- The actual model tool call is persisted before execution. Its result and any
-- receipt commit together; a new attempt resumes this boundary after disconnect.
CREATE TABLE agent_operations (
    operation_id TEXT PRIMARY KEY NOT NULL,
    input_id TEXT NOT NULL REFERENCES turns(id) ON DELETE CASCADE,
    call_id TEXT NOT NULL,
    name TEXT NOT NULL,
    arguments TEXT NOT NULL,
    result TEXT,
    receipt_id TEXT REFERENCES receipts(request_id),
    created_at INTEGER NOT NULL,
    UNIQUE(input_id,call_id)
) STRICT;
CREATE INDEX agent_operations_input ON agent_operations(input_id,created_at);

-- Receipts and their original captures outlive deleted conversation messages.
-- No conversation FK: this stable grouping is also the whole-input undo handle.
ALTER TABLE receipts ADD COLUMN logical_input_id TEXT;
ALTER TABLE receipts ADD COLUMN before_memberships TEXT;
ALTER TABLE receipts ADD COLUMN after_memberships TEXT;
CREATE INDEX receipts_logical_input ON receipts(logical_input_id,created_at);

PRAGMA user_version = 16;
