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
