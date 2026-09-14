-- Initial MemoryStore schema. Applied atomically by the core.

CREATE TABLE captures (
    id TEXT PRIMARY KEY NOT NULL,
    request_id TEXT NOT NULL UNIQUE,
    fingerprint BLOB NOT NULL,
    text TEXT,
    source TEXT,
    created_at INTEGER NOT NULL,
    CHECK ((text IS NULL) = (source IS NULL))
) STRICT;

CREATE TABLE capture_state (
    capture_id TEXT PRIMARY KEY NOT NULL REFERENCES captures(id),
    availability TEXT NOT NULL DEFAULT 'active' CHECK(availability IN ('active','trashed','purged')),
    understanding TEXT NOT NULL DEFAULT 'pending' CHECK(understanding IN ('pending','attached','deferred')),
    trash_owner TEXT REFERENCES memories(id)
) STRICT;

CREATE TABLE memory_versions (
    id TEXT PRIMARY KEY NOT NULL,
    memory_id TEXT NOT NULL REFERENCES memories(id),
    parent_id TEXT REFERENCES memory_versions(id),
    title TEXT,
    body TEXT,
    actor TEXT NOT NULL CHECK(actor IN ('user','ai')),
    reason TEXT NOT NULL CHECK(reason IN ('create','append','edit','restore','undo')),
    created_at INTEGER NOT NULL,
    review_kind TEXT CHECK(review_kind IS NULL OR review_kind = 'cleanup'),
    CHECK ((title IS NULL) = (body IS NULL))
) STRICT;

CREATE TABLE version_captures (
    version_id TEXT NOT NULL REFERENCES memory_versions(id),
    capture_id TEXT NOT NULL REFERENCES captures(id),
    PRIMARY KEY(version_id, capture_id)
) STRICT;

CREATE TABLE receipts (
    request_id TEXT PRIMARY KEY NOT NULL,
    fingerprint BLOB NOT NULL,
    action TEXT NOT NULL,
    capture_id TEXT REFERENCES captures(id),
    memory_id TEXT REFERENCES memories(id),
    before_version TEXT REFERENCES memory_versions(id),
    after_version TEXT REFERENCES memory_versions(id),
    status TEXT NOT NULL CHECK(status IN ('applied','needs_review','undone')),
    created_at INTEGER NOT NULL,
    logical_input_id TEXT,
    before_memberships TEXT,
    after_memberships TEXT
) STRICT;

CREATE TABLE capture_citations (
    capture_id TEXT NOT NULL REFERENCES captures(id),
    kind TEXT NOT NULL CHECK(kind IN ('capture','version')),
    source_id TEXT NOT NULL,
    PRIMARY KEY(capture_id,kind,source_id)
) STRICT;

CREATE TABLE conversations (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    draft TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    summary TEXT NOT NULL DEFAULT '',
    summary_through_seq INTEGER NOT NULL DEFAULT 0,
    memory_paused INTEGER NOT NULL DEFAULT 0 CHECK(memory_paused IN (0,1)),
    title_generated INTEGER NOT NULL DEFAULT 0 CHECK(title_generated IN (0,1))
) STRICT;

CREATE TABLE turns (
    id TEXT PRIMARY KEY NOT NULL,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    fingerprint BLOB NOT NULL,
    active_attempt TEXT,
    protocol_messages TEXT NOT NULL DEFAULT '[]',
    focused_memory_ids TEXT NOT NULL DEFAULT '[]',
    input_origin TEXT,
    checkpoint_text_chars INTEGER NOT NULL DEFAULT 0,
    follow_ups TEXT NOT NULL DEFAULT '[]',
    progress TEXT,
    record_only INTEGER NOT NULL DEFAULT 0 CHECK(record_only IN (0,1)),
    maintenance_paused INTEGER NOT NULL DEFAULT 0 CHECK(maintenance_paused IN (0,1))
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

CREATE TABLE message_citations (
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK(kind IN ('capture','version')),
    source_id TEXT NOT NULL,
    cited INTEGER NOT NULL DEFAULT 0 CHECK(cited IN (0,1)),
    excerpt_start INTEGER NOT NULL DEFAULT 0,
    excerpt_length INTEGER NOT NULL DEFAULT 1800,
    PRIMARY KEY(message_id,kind,source_id)
) STRICT;

CREATE TABLE workspace_drafts (
    key TEXT PRIMARY KEY NOT NULL,
    payload TEXT NOT NULL
) STRICT;

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

CREATE TABLE collection_entries (
    collection_id TEXT NOT NULL REFERENCES collections(id),
    kind TEXT NOT NULL CHECK(kind IN ('memory','capture')),
    record_id TEXT NOT NULL,
    PRIMARY KEY(collection_id,kind,record_id)
) STRICT;

CREATE TABLE conversation_collections (
    conversation_id TEXT PRIMARY KEY NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    collection_id TEXT NOT NULL REFERENCES collections(id)
) STRICT;

CREATE TABLE collection_feedback (
    receipt_id TEXT PRIMARY KEY NOT NULL REFERENCES receipts(request_id) ON DELETE CASCADE,
    suggestions TEXT NOT NULL DEFAULT '[]',
    dismissed INTEGER NOT NULL DEFAULT 0 CHECK(dismissed IN (0,1))
) STRICT;

CREATE TABLE "memories" (
    id TEXT PRIMARY KEY NOT NULL,
    current_version_id TEXT REFERENCES memory_versions(id) DEFERRABLE INITIALLY DEFERRED,
    state TEXT NOT NULL DEFAULT 'active' CHECK(state IN ('active','trashed','undone','merged','purged')),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

CREATE TABLE "receipt_changes" (
    request_id TEXT NOT NULL REFERENCES receipts(request_id),
    memory_id TEXT NOT NULL REFERENCES memories(id),
    before_version TEXT REFERENCES memory_versions(id),
    after_version TEXT NOT NULL REFERENCES memory_versions(id),
    before_state TEXT NOT NULL CHECK(before_state IN ('active','undone','merged')),
    after_state TEXT NOT NULL CHECK(after_state IN ('active','undone','merged')),
    PRIMARY KEY(request_id,memory_id)
) STRICT;

CREATE TABLE organization_jobs (
    memory_id TEXT PRIMARY KEY NOT NULL REFERENCES memories(id),
    input_version_id TEXT NOT NULL REFERENCES memory_versions(id),
    capture_id TEXT NOT NULL REFERENCES captures(id),
    attempt_id TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL CHECK(status IN ('pending','processing','done','failed','deferred','paused')),
    reason TEXT NOT NULL DEFAULT '',
    receipt_id TEXT,
    created_at INTEGER NOT NULL,
    reason_code TEXT
) STRICT;

CREATE TABLE memory_keywords (
    version_id TEXT PRIMARY KEY NOT NULL REFERENCES memory_versions(id),
    terms TEXT NOT NULL
) STRICT;

CREATE VIRTUAL TABLE record_fts USING fts5(kind UNINDEXED,source_id UNINDEXED,title,body,origin,tokenize='trigram');

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

CREATE TABLE library_revision (
    id INTEGER PRIMARY KEY CHECK(id=1),
    revision INTEGER NOT NULL
) STRICT;

CREATE TABLE ui_change_epoch (
    id INTEGER PRIMARY KEY CHECK(id=1),
    epoch TEXT NOT NULL
) STRICT;

CREATE TABLE ui_changes (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    domain TEXT NOT NULL,
    entity TEXT NOT NULL
) STRICT;

CREATE TABLE "embedding_chunks" (
    memory_id TEXT NOT NULL REFERENCES memories(id),
    version_id TEXT NOT NULL REFERENCES memory_versions(id),
    ordinal INTEGER NOT NULL,
    start_char INTEGER NOT NULL CHECK(start_char>=0),
    end_char INTEGER NOT NULL CHECK(end_char>start_char),
    input_hash TEXT NOT NULL,
    vector BLOB NOT NULL CHECK(typeof(vector)='blob' AND length(vector)>=4 AND length(vector)<=65536 AND length(vector)%4=0),
    PRIMARY KEY(memory_id,ordinal)
) STRICT;

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

CREATE INDEX versions_by_memory ON memory_versions(memory_id, created_at);

CREATE INDEX versions_by_capture ON version_captures(capture_id);

CREATE UNIQUE INDEX one_running_answer ON messages(conversation_id) WHERE status='processing';

CREATE INDEX conversation_messages ON messages(conversation_id,seq);

CREATE INDEX captures_recent ON captures(created_at DESC, id);

CREATE UNIQUE INDEX collection_names ON collections(name COLLATE NOCASE) WHERE archived=0;

CREATE INDEX collections_by_record ON collection_entries(kind,record_id,collection_id);

CREATE INDEX memories_recent ON memories(state, updated_at DESC, id);

CREATE INDEX receipt_changes_by_memory ON receipt_changes(memory_id,request_id);

CREATE INDEX organization_pending ON organization_jobs(status,created_at,memory_id);

CREATE INDEX embedding_input_hash ON embedding_chunks(memory_id,input_hash);

CREATE INDEX agent_operations_input ON agent_operations(input_id,created_at);

CREATE INDEX receipts_logical_input ON receipts(logical_input_id,created_at);

CREATE TRIGGER captures_immutable BEFORE UPDATE ON captures
WHEN NEW.id IS NOT OLD.id OR NEW.request_id IS NOT OLD.request_id
 OR NEW.fingerprint IS NOT OLD.fingerprint OR NEW.created_at IS NOT OLD.created_at
 OR NEW.text IS NOT NULL OR NEW.source IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'capture content is immutable; explicit erasure only'); END;

CREATE TRIGGER versions_immutable BEFORE UPDATE ON memory_versions
WHEN NEW.id IS NOT OLD.id OR NEW.memory_id IS NOT OLD.memory_id
 OR NEW.parent_id IS NOT OLD.parent_id OR NEW.actor IS NOT OLD.actor
 OR NEW.reason IS NOT OLD.reason OR NEW.created_at IS NOT OLD.created_at
 OR NEW.title IS NOT NULL OR NEW.body IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'versions are immutable; explicit erasure only'); END;

CREATE TRIGGER version_parent BEFORE INSERT ON memory_versions
WHEN NEW.parent_id IS NOT NULL AND NOT EXISTS (
 SELECT 1 FROM memory_versions WHERE id=NEW.parent_id AND memory_id=NEW.memory_id
)
BEGIN SELECT RAISE(ABORT, 'version parent belongs to another memory'); END;

CREATE TRIGGER remove_purged_capture_navigation AFTER UPDATE OF availability ON capture_state
WHEN NEW.availability='purged'
BEGIN
    DELETE FROM record_pins WHERE kind='capture' AND record_id=NEW.capture_id;
    DELETE FROM collection_entries WHERE kind='capture' AND record_id=NEW.capture_id;
END;

CREATE TRIGGER remove_purged_capture_feedback AFTER UPDATE OF availability ON capture_state
WHEN NEW.availability='purged'
BEGIN
    DELETE FROM collection_feedback WHERE receipt_id IN (SELECT request_id FROM receipts WHERE capture_id=NEW.capture_id);
END;

CREATE TRIGGER version_review_kind_immutable BEFORE UPDATE OF review_kind ON memory_versions
WHEN NEW.review_kind IS NOT OLD.review_kind
BEGIN SELECT RAISE(ABORT, 'version review provenance is immutable'); END;

CREATE TRIGGER memory_head BEFORE UPDATE OF current_version_id ON memories
WHEN NEW.current_version_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM memory_versions WHERE id=NEW.current_version_id AND memory_id=NEW.id
)
BEGIN SELECT RAISE(ABORT, 'current version belongs to another memory'); END;

CREATE TRIGGER remove_purged_memory_navigation AFTER UPDATE OF state ON memories
WHEN NEW.state='purged'
BEGIN
    DELETE FROM record_pins WHERE kind='memory' AND record_id=NEW.id;
    DELETE FROM collection_entries WHERE kind='memory' AND record_id=NEW.id;
END;

CREATE TRIGGER remove_purged_memory_feedback AFTER UPDATE OF state ON memories
WHEN NEW.state='purged'
BEGIN
    DELETE FROM collection_feedback WHERE receipt_id IN (SELECT request_id FROM receipts WHERE memory_id=NEW.id);
END;

CREATE TRIGGER organization_hidden AFTER UPDATE OF state ON memories
WHEN NEW.state!='active'
BEGIN
    UPDATE organization_jobs SET status='paused' WHERE memory_id=NEW.id AND status IN ('pending','processing');
END;

CREATE TRIGGER organization_erased AFTER UPDATE OF state ON memories
WHEN NEW.state='purged'
BEGIN
    DELETE FROM organization_jobs WHERE memory_id=NEW.id;
END;

CREATE TRIGGER memory_search_head AFTER UPDATE OF current_version_id,state ON memories
BEGIN
    DELETE FROM record_fts WHERE source_id=OLD.current_version_id;
    INSERT INTO record_fts(kind,source_id,title,body,origin)
    SELECT 'version',v.id,v.title,v.body,
        COALESCE((SELECT group_concat(c.source,' ') FROM version_captures vc JOIN captures c ON c.id=vc.capture_id WHERE vc.version_id=v.id),'') || ' ' || COALESCE(k.terms,'')
    FROM memory_versions v LEFT JOIN memory_keywords k ON k.version_id=v.id
    WHERE v.id=NEW.current_version_id AND NEW.state='active' AND v.body IS NOT NULL;
END;

CREATE TRIGGER memory_search_erase AFTER UPDATE OF body ON memory_versions
WHEN NEW.body IS NULL
BEGIN
    DELETE FROM record_fts WHERE source_id=NEW.id;
    DELETE FROM memory_keywords WHERE version_id=NEW.id;
END;

CREATE TRIGGER memory_search_metadata AFTER UPDATE OF source ON captures
BEGIN
    UPDATE record_fts SET origin=COALESCE((SELECT group_concat(c.source,' ') FROM version_captures vc JOIN captures c ON c.id=vc.capture_id WHERE vc.version_id=record_fts.source_id),'') || ' ' || COALESCE((SELECT terms FROM memory_keywords WHERE version_id=record_fts.source_id),'')
    WHERE source_id IN (SELECT version_id FROM version_captures WHERE capture_id=NEW.id);
END;

CREATE TRIGGER memory_search_keywords AFTER INSERT ON memory_keywords
BEGIN
    UPDATE record_fts SET origin=COALESCE((SELECT group_concat(c.source,' ') FROM version_captures vc JOIN captures c ON c.id=vc.capture_id WHERE vc.version_id=NEW.version_id),'') || ' ' || NEW.terms
    WHERE source_id=NEW.version_id;
END;

CREATE TRIGGER library_revision_captures_insert AFTER INSERT ON captures
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_captures_update AFTER UPDATE ON captures
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_captures_delete AFTER DELETE ON captures
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_capture_state_insert AFTER INSERT ON capture_state
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_capture_state_update AFTER UPDATE ON capture_state
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_capture_state_delete AFTER DELETE ON capture_state
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_memories_insert AFTER INSERT ON memories
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_memories_update AFTER UPDATE ON memories
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_memories_delete AFTER DELETE ON memories
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_memory_versions_insert AFTER INSERT ON memory_versions
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_memory_versions_update AFTER UPDATE ON memory_versions
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_memory_versions_delete AFTER DELETE ON memory_versions
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_version_captures_insert AFTER INSERT ON version_captures
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_version_captures_update AFTER UPDATE ON version_captures
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_version_captures_delete AFTER DELETE ON version_captures
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_receipts_insert AFTER INSERT ON receipts
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_receipts_update AFTER UPDATE ON receipts
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_receipts_delete AFTER DELETE ON receipts
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_receipt_changes_insert AFTER INSERT ON receipt_changes
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_receipt_changes_update AFTER UPDATE ON receipt_changes
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_receipt_changes_delete AFTER DELETE ON receipt_changes
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_capture_citations_insert AFTER INSERT ON capture_citations
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_capture_citations_update AFTER UPDATE ON capture_citations
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_capture_citations_delete AFTER DELETE ON capture_citations
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_conversations_insert AFTER INSERT ON conversations
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_conversations_update AFTER UPDATE ON conversations
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_conversations_delete AFTER DELETE ON conversations
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_turns_insert AFTER INSERT ON turns
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_turns_update AFTER UPDATE ON turns
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_turns_delete AFTER DELETE ON turns
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_messages_insert AFTER INSERT ON messages
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_messages_update AFTER UPDATE ON messages
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_messages_delete AFTER DELETE ON messages
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_message_citations_insert AFTER INSERT ON message_citations
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_message_citations_update AFTER UPDATE ON message_citations
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_message_citations_delete AFTER DELETE ON message_citations
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_message_evidence_spans_insert AFTER INSERT ON message_evidence_spans
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_message_evidence_spans_update AFTER UPDATE ON message_evidence_spans
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_message_evidence_spans_delete AFTER DELETE ON message_evidence_spans
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_organization_jobs_insert AFTER INSERT ON organization_jobs
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_organization_jobs_update AFTER UPDATE ON organization_jobs
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_organization_jobs_delete AFTER DELETE ON organization_jobs
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_record_pins_insert AFTER INSERT ON record_pins
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_record_pins_update AFTER UPDATE ON record_pins
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_record_pins_delete AFTER DELETE ON record_pins
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_collections_insert AFTER INSERT ON collections
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_collections_update AFTER UPDATE ON collections
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_collections_delete AFTER DELETE ON collections
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_collection_entries_insert AFTER INSERT ON collection_entries
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_collection_entries_update AFTER UPDATE ON collection_entries
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_collection_entries_delete AFTER DELETE ON collection_entries
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_conversation_collections_insert AFTER INSERT ON conversation_collections
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_conversation_collections_update AFTER UPDATE ON conversation_collections
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_conversation_collections_delete AFTER DELETE ON conversation_collections
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_collection_feedback_insert AFTER INSERT ON collection_feedback
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_collection_feedback_update AFTER UPDATE ON collection_feedback
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER library_revision_collection_feedback_delete AFTER DELETE ON collection_feedback
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;

CREATE TRIGGER ui_changes_bound AFTER INSERT ON ui_changes BEGIN
 DELETE FROM ui_changes WHERE seq <= NEW.seq - 8192;
END;

CREATE TRIGGER ui_change_captures_insert AFTER INSERT ON captures BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('capture:'||NEW.id,'*'));
END;

CREATE TRIGGER ui_change_captures_update AFTER UPDATE ON captures BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('capture:'||OLD.id,'*'));
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('capture:'||NEW.id,'*'));
END;

CREATE TRIGGER ui_change_captures_delete AFTER DELETE ON captures BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('capture:'||OLD.id,'*'));
END;

CREATE TRIGGER ui_change_capture_state_insert AFTER INSERT ON capture_state BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('capture:'||NEW.capture_id,'*'));
END;

CREATE TRIGGER ui_change_capture_state_update AFTER UPDATE ON capture_state BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('capture:'||OLD.capture_id,'*'));
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('capture:'||NEW.capture_id,'*'));
END;

CREATE TRIGGER ui_change_capture_state_delete AFTER DELETE ON capture_state BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('capture:'||OLD.capture_id,'*'));
END;

CREATE TRIGGER ui_change_memories_insert AFTER INSERT ON memories BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('memory:'||NEW.id,'*'));
END;

CREATE TRIGGER ui_change_memories_update AFTER UPDATE ON memories BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('memory:'||OLD.id,'*'));
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('memory:'||NEW.id,'*'));
END;

CREATE TRIGGER ui_change_memories_delete AFTER DELETE ON memories BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('memory:'||OLD.id,'*'));
END;

CREATE TRIGGER ui_change_memory_versions_insert AFTER INSERT ON memory_versions BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('memory:'||NEW.memory_id,'*'));
END;

CREATE TRIGGER ui_change_memory_versions_update AFTER UPDATE ON memory_versions BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('memory:'||OLD.memory_id,'*'));
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('memory:'||NEW.memory_id,'*'));
END;

CREATE TRIGGER ui_change_memory_versions_delete AFTER DELETE ON memory_versions BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('memory:'||OLD.memory_id,'*'));
END;

CREATE TRIGGER ui_change_version_captures_insert AFTER INSERT ON version_captures BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('memory:'||(SELECT memory_id FROM memory_versions WHERE id=NEW.version_id),'*'));
END;

CREATE TRIGGER ui_change_version_captures_update AFTER UPDATE ON version_captures BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('memory:'||(SELECT memory_id FROM memory_versions WHERE id=OLD.version_id),'*'));
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('memory:'||(SELECT memory_id FROM memory_versions WHERE id=NEW.version_id),'*'));
END;

CREATE TRIGGER ui_change_version_captures_delete AFTER DELETE ON version_captures BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('memory:'||(SELECT memory_id FROM memory_versions WHERE id=OLD.version_id),'*'));
END;

CREATE TRIGGER ui_change_receipts_insert AFTER INSERT ON receipts BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('organization',COALESCE('memory:'||NEW.memory_id,'*'));
END;

CREATE TRIGGER ui_change_receipts_update AFTER UPDATE ON receipts BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('organization',COALESCE('memory:'||OLD.memory_id,'*'));
 INSERT INTO ui_changes(domain,entity) VALUES ('organization',COALESCE('memory:'||NEW.memory_id,'*'));
END;

CREATE TRIGGER ui_change_receipts_delete AFTER DELETE ON receipts BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('organization',COALESCE('memory:'||OLD.memory_id,'*'));
END;

CREATE TRIGGER ui_change_receipt_changes_insert AFTER INSERT ON receipt_changes BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('organization',COALESCE('memory:'||NEW.memory_id,'*'));
END;

CREATE TRIGGER ui_change_receipt_changes_update AFTER UPDATE ON receipt_changes BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('organization',COALESCE('memory:'||OLD.memory_id,'*'));
 INSERT INTO ui_changes(domain,entity) VALUES ('organization',COALESCE('memory:'||NEW.memory_id,'*'));
END;

CREATE TRIGGER ui_change_receipt_changes_delete AFTER DELETE ON receipt_changes BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('organization',COALESCE('memory:'||OLD.memory_id,'*'));
END;

CREATE TRIGGER ui_change_capture_citations_insert AFTER INSERT ON capture_citations BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('capture:'||NEW.capture_id,'*'));
END;

CREATE TRIGGER ui_change_capture_citations_update AFTER UPDATE ON capture_citations BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('capture:'||OLD.capture_id,'*'));
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('capture:'||NEW.capture_id,'*'));
END;

CREATE TRIGGER ui_change_capture_citations_delete AFTER DELETE ON capture_citations BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('capture:'||OLD.capture_id,'*'));
END;

CREATE TRIGGER ui_change_conversations_insert AFTER INSERT ON conversations BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('discussion',COALESCE(NEW.id,'*'));
END;

CREATE TRIGGER ui_change_conversations_update AFTER UPDATE ON conversations BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('discussion',COALESCE(OLD.id,'*'));
 INSERT INTO ui_changes(domain,entity) VALUES ('discussion',COALESCE(NEW.id,'*'));
END;

CREATE TRIGGER ui_change_conversations_delete AFTER DELETE ON conversations BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('discussion',COALESCE(OLD.id,'*'));
END;

CREATE TRIGGER ui_change_messages_insert AFTER INSERT ON messages BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('discussion',COALESCE(NEW.conversation_id,'*'));
END;

CREATE TRIGGER ui_change_messages_update AFTER UPDATE ON messages BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('discussion',COALESCE(OLD.conversation_id,'*'));
 INSERT INTO ui_changes(domain,entity) VALUES ('discussion',COALESCE(NEW.conversation_id,'*'));
END;

CREATE TRIGGER ui_change_messages_delete AFTER DELETE ON messages BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('discussion',COALESCE(OLD.conversation_id,'*'));
END;

CREATE TRIGGER ui_change_message_citations_insert AFTER INSERT ON message_citations BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('discussion',COALESCE((SELECT conversation_id FROM messages WHERE id=NEW.message_id),'*'));
END;

CREATE TRIGGER ui_change_message_citations_update AFTER UPDATE ON message_citations BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('discussion',COALESCE((SELECT conversation_id FROM messages WHERE id=OLD.message_id),'*'));
 INSERT INTO ui_changes(domain,entity) VALUES ('discussion',COALESCE((SELECT conversation_id FROM messages WHERE id=NEW.message_id),'*'));
END;

CREATE TRIGGER ui_change_message_citations_delete AFTER DELETE ON message_citations BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('discussion',COALESCE((SELECT conversation_id FROM messages WHERE id=OLD.message_id),'*'));
END;

CREATE TRIGGER ui_change_message_evidence_spans_insert AFTER INSERT ON message_evidence_spans BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('discussion',COALESCE((SELECT conversation_id FROM messages WHERE id=NEW.message_id),'*'));
END;

CREATE TRIGGER ui_change_message_evidence_spans_update AFTER UPDATE ON message_evidence_spans BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('discussion',COALESCE((SELECT conversation_id FROM messages WHERE id=OLD.message_id),'*'));
 INSERT INTO ui_changes(domain,entity) VALUES ('discussion',COALESCE((SELECT conversation_id FROM messages WHERE id=NEW.message_id),'*'));
END;

CREATE TRIGGER ui_change_message_evidence_spans_delete AFTER DELETE ON message_evidence_spans BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('discussion',COALESCE((SELECT conversation_id FROM messages WHERE id=OLD.message_id),'*'));
END;

CREATE TRIGGER ui_change_organization_jobs_insert AFTER INSERT ON organization_jobs BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('organization',COALESCE('memory:'||NEW.memory_id,'*'));
END;

CREATE TRIGGER ui_change_organization_jobs_update AFTER UPDATE ON organization_jobs BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('organization',COALESCE('memory:'||OLD.memory_id,'*'));
 INSERT INTO ui_changes(domain,entity) VALUES ('organization',COALESCE('memory:'||NEW.memory_id,'*'));
END;

CREATE TRIGGER ui_change_organization_jobs_delete AFTER DELETE ON organization_jobs BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('organization',COALESCE('memory:'||OLD.memory_id,'*'));
END;

CREATE TRIGGER ui_change_record_pins_insert AFTER INSERT ON record_pins BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('navigation',COALESCE(NEW.kind||':'||NEW.record_id,'*'));
END;

CREATE TRIGGER ui_change_record_pins_update AFTER UPDATE ON record_pins BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('navigation',COALESCE(OLD.kind||':'||OLD.record_id,'*'));
 INSERT INTO ui_changes(domain,entity) VALUES ('navigation',COALESCE(NEW.kind||':'||NEW.record_id,'*'));
END;

CREATE TRIGGER ui_change_record_pins_delete AFTER DELETE ON record_pins BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('navigation',COALESCE(OLD.kind||':'||OLD.record_id,'*'));
END;

CREATE TRIGGER ui_change_collections_insert AFTER INSERT ON collections BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('collection',COALESCE(NEW.id,'*'));
END;

CREATE TRIGGER ui_change_collections_update AFTER UPDATE ON collections BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('collection',COALESCE(OLD.id,'*'));
 INSERT INTO ui_changes(domain,entity) VALUES ('collection',COALESCE(NEW.id,'*'));
END;

CREATE TRIGGER ui_change_collections_delete AFTER DELETE ON collections BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('collection',COALESCE(OLD.id,'*'));
END;

CREATE TRIGGER ui_change_collection_entries_insert AFTER INSERT ON collection_entries BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('collection',COALESCE(NEW.collection_id,'*'));
 INSERT INTO ui_changes(domain,entity) VALUES ('navigation',COALESCE(NEW.kind||':'||NEW.record_id,'*'));
END;

CREATE TRIGGER ui_change_collection_entries_update AFTER UPDATE ON collection_entries BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('collection',COALESCE(OLD.collection_id,'*'));
 INSERT INTO ui_changes(domain,entity) VALUES ('navigation',COALESCE(OLD.kind||':'||OLD.record_id,'*'));
 INSERT INTO ui_changes(domain,entity) VALUES ('collection',COALESCE(NEW.collection_id,'*'));
 INSERT INTO ui_changes(domain,entity) VALUES ('navigation',COALESCE(NEW.kind||':'||NEW.record_id,'*'));
END;

CREATE TRIGGER ui_change_collection_entries_delete AFTER DELETE ON collection_entries BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('collection',COALESCE(OLD.collection_id,'*'));
 INSERT INTO ui_changes(domain,entity) VALUES ('navigation',COALESCE(OLD.kind||':'||OLD.record_id,'*'));
END;

CREATE TRIGGER ui_change_conversation_collections_insert AFTER INSERT ON conversation_collections BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('discussion',COALESCE(NEW.conversation_id,'*'));
END;

CREATE TRIGGER ui_change_conversation_collections_update AFTER UPDATE ON conversation_collections BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('discussion',COALESCE(OLD.conversation_id,'*'));
 INSERT INTO ui_changes(domain,entity) VALUES ('discussion',COALESCE(NEW.conversation_id,'*'));
END;

CREATE TRIGGER ui_change_conversation_collections_delete AFTER DELETE ON conversation_collections BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('discussion',COALESCE(OLD.conversation_id,'*'));
END;

CREATE TRIGGER ui_change_collection_feedback_insert AFTER INSERT ON collection_feedback BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('organization',COALESCE('memory:'||(SELECT memory_id FROM receipts WHERE request_id=NEW.receipt_id),'*'));
END;

CREATE TRIGGER ui_change_collection_feedback_update AFTER UPDATE ON collection_feedback BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('organization',COALESCE('memory:'||(SELECT memory_id FROM receipts WHERE request_id=OLD.receipt_id),'*'));
 INSERT INTO ui_changes(domain,entity) VALUES ('organization',COALESCE('memory:'||(SELECT memory_id FROM receipts WHERE request_id=NEW.receipt_id),'*'));
END;

CREATE TRIGGER ui_change_collection_feedback_delete AFTER DELETE ON collection_feedback BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('organization',COALESCE('memory:'||(SELECT memory_id FROM receipts WHERE request_id=OLD.receipt_id),'*'));
END;

CREATE TRIGGER embedding_erased AFTER UPDATE OF body ON memory_versions WHEN NEW.body IS NULL
BEGIN
    DELETE FROM embedding_chunks WHERE version_id=NEW.id;
    DELETE FROM embedding_records WHERE version_id=NEW.id;
END;

INSERT INTO library_revision VALUES (1, 0);
INSERT INTO ui_change_epoch VALUES (1, lower(hex(randomblob(16))));
INSERT INTO record_fts(record_fts,rank) VALUES ('secure-delete',1);
PRAGMA user_version = 1;
