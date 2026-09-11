-- UI-visible changes only. Draft autosave and derived search/vector indexes
-- have their own synchronization/status channels and must not refresh the library.
CREATE TABLE library_revision (
    id INTEGER PRIMARY KEY CHECK(id=1),
    revision INTEGER NOT NULL
) STRICT;
INSERT INTO library_revision VALUES(1,0);

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
CREATE TRIGGER library_revision_conclusion_intents_insert AFTER INSERT ON conclusion_intents
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER library_revision_conclusion_intents_update AFTER UPDATE ON conclusion_intents
BEGIN UPDATE library_revision SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER library_revision_conclusion_intents_delete AFTER DELETE ON conclusion_intents
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

PRAGMA user_version=12;
