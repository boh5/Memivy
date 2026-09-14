-- Rebuild the current-memory search index.
DROP TRIGGER IF EXISTS memory_search_head;
DROP TRIGGER IF EXISTS memory_search_erase;
DROP TRIGGER IF EXISTS memory_search_metadata;
DROP TRIGGER IF EXISTS memory_search_keywords;
DROP TABLE IF EXISTS record_fts;
CREATE VIRTUAL TABLE record_fts USING fts5(kind UNINDEXED,source_id UNINDEXED,title,body,origin,tokenize='trigram');
INSERT INTO record_fts(record_fts,rank) VALUES('secure-delete',1);
INSERT INTO record_fts(kind,source_id,title,body,origin)
SELECT 'version',v.id,v.title,v.body,
    COALESCE((SELECT group_concat(c.source,' ') FROM version_captures vc JOIN captures c ON c.id=vc.capture_id WHERE vc.version_id=v.id),'') || ' ' || COALESCE(k.terms,'')
FROM memories m JOIN memory_versions v ON v.id=m.current_version_id LEFT JOIN memory_keywords k ON k.version_id=v.id
WHERE m.state='active' AND v.body IS NOT NULL;
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
