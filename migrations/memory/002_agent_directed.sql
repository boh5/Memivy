-- Semantic maintenance is now explicit work by the current conversation Agent.
-- Saved memories, originals, versions and receipts remain intact.
DROP TRIGGER remove_purged_capture_feedback;
DROP TRIGGER remove_purged_memory_feedback;
DROP TRIGGER organization_hidden;
DROP TRIGGER organization_erased;
DROP TABLE collection_feedback;
DROP TABLE organization_jobs;

DROP TRIGGER ui_change_receipts_insert;
CREATE TRIGGER ui_change_receipts_insert AFTER INSERT ON receipts BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('memory:'||NEW.memory_id,'*'));
END;

DROP TRIGGER ui_change_receipts_update;
CREATE TRIGGER ui_change_receipts_update AFTER UPDATE ON receipts BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('memory:'||OLD.memory_id,'*'));
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('memory:'||NEW.memory_id,'*'));
END;

DROP TRIGGER ui_change_receipts_delete;
CREATE TRIGGER ui_change_receipts_delete AFTER DELETE ON receipts BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('memory:'||OLD.memory_id,'*'));
END;

DROP TRIGGER ui_change_receipt_changes_insert;
CREATE TRIGGER ui_change_receipt_changes_insert AFTER INSERT ON receipt_changes BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('memory:'||NEW.memory_id,'*'));
END;

DROP TRIGGER ui_change_receipt_changes_update;
CREATE TRIGGER ui_change_receipt_changes_update AFTER UPDATE ON receipt_changes BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('memory:'||OLD.memory_id,'*'));
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('memory:'||NEW.memory_id,'*'));
END;

DROP TRIGGER ui_change_receipt_changes_delete;
CREATE TRIGGER ui_change_receipt_changes_delete AFTER DELETE ON receipt_changes BEGIN
 INSERT INTO ui_changes(domain,entity) VALUES ('memory',COALESCE('memory:'||OLD.memory_id,'*'));
END;

-- Navigation snapshots are per affected memory, including both sides of a merge.
ALTER TABLE receipt_changes ADD COLUMN navigation_before TEXT;
ALTER TABLE receipt_changes ADD COLUMN navigation_after TEXT;
-- Previous conversational changes recorded collection membership, but never pin changes.
UPDATE receipt_changes SET
 navigation_before=(SELECT json_object('collections',json(before_memberships),'pinned',NULL) FROM receipts WHERE request_id=receipt_changes.request_id),
 navigation_after=(SELECT json_object('collections',json(after_memberships),'pinned',NULL) FROM receipts WHERE request_id=receipt_changes.request_id)
 WHERE request_id IN (SELECT request_id FROM receipts WHERE logical_input_id IS NOT NULL AND action!='undo');
-- Old merges allowed only unpinned, uncollected sources and left target navigation unchanged.
UPDATE receipt_changes SET navigation_before='{"collections":[],"pinned":false}', navigation_after='{"collections":[],"pinned":false}'
 WHERE after_state='merged' AND request_id IN (SELECT request_id FROM receipts WHERE action='merge');
ALTER TABLE receipts DROP COLUMN before_memberships;
ALTER TABLE receipts DROP COLUMN after_memberships;

ALTER TABLE receipts ADD COLUMN reason TEXT;

-- Collection receipts share the durable logical input group with memory changes.
ALTER TABLE receipts ADD COLUMN collection_changes TEXT NOT NULL DEFAULT '[]' CHECK(json_valid(collection_changes));

-- Every entry point, including manual navigation, merge and undo, advances the
-- collection revision when its actual membership or visible member count changes.
CREATE TRIGGER collection_member_insert_revision AFTER INSERT ON collection_entries BEGIN
 UPDATE collections SET revision=revision+1 WHERE id=NEW.collection_id;
END;
CREATE TRIGGER collection_member_delete_revision AFTER DELETE ON collection_entries BEGIN
 UPDATE collections SET revision=revision+1 WHERE id=OLD.collection_id;
END;
CREATE TRIGGER collection_member_update_revision AFTER UPDATE ON collection_entries
WHEN OLD.collection_id!=NEW.collection_id OR OLD.kind!=NEW.kind OR OLD.record_id!=NEW.record_id BEGIN
 UPDATE collections SET revision=revision+1 WHERE id IN (OLD.collection_id,NEW.collection_id);
END;
CREATE TRIGGER collection_memory_state_revision AFTER UPDATE OF state ON memories
WHEN (OLD.state='active')!=(NEW.state='active') BEGIN
 UPDATE collections SET revision=revision+1 WHERE id IN
 (SELECT collection_id FROM collection_entries WHERE kind='memory' AND record_id=NEW.id);
END;

CREATE TRIGGER ui_change_collection_receipts_insert AFTER INSERT ON receipts BEGIN
 INSERT INTO ui_changes(domain,entity)
 SELECT 'collection',json_extract(value,'$.collection_id') FROM json_each(NEW.collection_changes);
 INSERT INTO ui_changes(domain,entity)
 SELECT 'discussion',conversation_id FROM turns WHERE id=NEW.logical_input_id;
END;
CREATE TRIGGER ui_change_collection_receipts_update AFTER UPDATE OF status,collection_changes,logical_input_id ON receipts BEGIN
 INSERT INTO ui_changes(domain,entity)
 SELECT 'collection',json_extract(value,'$.collection_id') FROM json_each(OLD.collection_changes)
 UNION SELECT 'collection',json_extract(value,'$.collection_id') FROM json_each(NEW.collection_changes);
 INSERT INTO ui_changes(domain,entity)
 SELECT 'discussion',conversation_id FROM turns WHERE id IN (OLD.logical_input_id,NEW.logical_input_id);
END;
