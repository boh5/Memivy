-- Keep existing immutable version reasons and annotate explicitly reviewed AI edits.
ALTER TABLE memory_versions ADD COLUMN review_kind TEXT CHECK(review_kind IS NULL OR review_kind = 'cleanup');
CREATE TRIGGER version_review_kind_immutable BEFORE UPDATE OF review_kind ON memory_versions
WHEN NEW.review_kind IS NOT OLD.review_kind
BEGIN SELECT RAISE(ABORT, 'version review provenance is immutable'); END;
PRAGMA user_version = 8;
