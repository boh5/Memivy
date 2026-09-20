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
