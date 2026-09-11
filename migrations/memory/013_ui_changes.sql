-- Committed invalidation metadata only; no document bodies or credentials.
CREATE TABLE ui_change_epoch (id INTEGER PRIMARY KEY CHECK(id=1), epoch TEXT NOT NULL) STRICT;
INSERT INTO ui_change_epoch VALUES (1, lower(hex(randomblob(16))));
CREATE TABLE ui_changes (
 seq INTEGER PRIMARY KEY AUTOINCREMENT,
 domain TEXT NOT NULL,
 entity TEXT NOT NULL
) STRICT;
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
CREATE TRIGGER ui_change_conclusion_intents_insert AFTER INSERT ON conclusion_intents BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('capture:'||NEW.capture_id,'*'));
END;
CREATE TRIGGER ui_change_conclusion_intents_update AFTER UPDATE ON conclusion_intents BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('capture:'||OLD.capture_id,'*'));
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('capture:'||NEW.capture_id,'*'));
END;
CREATE TRIGGER ui_change_conclusion_intents_delete AFTER DELETE ON conclusion_intents BEGIN
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
PRAGMA user_version = 13;
