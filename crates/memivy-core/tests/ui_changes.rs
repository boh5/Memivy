use memivy_core::memory::*;
use rusqlite::Connection;
use uuid::Uuid;
fn capture(store: &MemoryStore) -> CaptureResult {
    store
        .capture(&CaptureRequest {
            request_id: Uuid::new_v4().to_string(),
            text: "synthetic UI change".into(),
            origin: Origin::User {
                app: "test".into(),
                project: None,
                uri: None,
            },
        })
        .unwrap()
}
#[test]
fn committed_changes_are_scoped_cross_connection_and_drafts_are_excluded() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let baseline = store.library_changes(None).unwrap();
    assert!(baseline.reset);
    let other = MemoryStore::open(dir.path()).unwrap();
    let saved = capture(&other);
    let changes = store.library_changes(Some(&baseline.cursor)).unwrap();
    assert!(!changes.reset);
    assert!(
        changes
            .changes
            .iter()
            .any(|c| c.domain == "memory" && c.entity == format!("memory:{}", saved.memory_id))
    );
    assert!(!changes.changes.iter().any(|c| c.domain == "discussion"));
    other
        .save_workspace_draft(&WorkspaceDraft {
            key: "input".into(),
            request_id: Uuid::new_v4().to_string(),
            title: "".into(),
            body: "draft only".into(),
            expected_version: None,
            origin: None,
            context: vec![],
            destination: None,
        })
        .unwrap();
    assert!(
        store
            .library_changes(Some(&changes.cursor))
            .unwrap()
            .changes
            .is_empty()
    );
    other
        .pin_record(
            &RecordKey {
                kind: "memory".into(),
                id: saved.memory_id.clone(),
            },
            true,
        )
        .unwrap();
    let pins = store.library_changes(Some(&changes.cursor)).unwrap();
    assert!(pins.changes.iter().all(|c| c.domain == "navigation"));
    let db = Connection::open(store.database_path()).unwrap();
    db.execute_batch("BEGIN; UPDATE memories SET state='trashed'; ROLLBACK;")
        .unwrap();
    assert!(
        store
            .library_changes(Some(&pins.cursor))
            .unwrap()
            .changes
            .is_empty()
    );
}
#[test]
fn bounded_journal_gaps_and_library_epochs_require_regional_revalidation() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let baseline = store.library_changes(None).unwrap();
    let db = Connection::open(store.database_path()).unwrap();
    db.execute_batch("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<8200) INSERT INTO ui_changes(domain,entity) SELECT 'memory','synthetic' FROM n;").unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM ui_changes", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        8192
    );
    let gap = store.library_changes(Some(&baseline.cursor)).unwrap();
    assert!(gap.reset);
    assert!(
        store
            .library_changes(Some(&gap.cursor))
            .unwrap()
            .changes
            .is_empty()
    );
    let foreign = ChangeCursor {
        epoch: "different library".into(),
        sequence: gap.cursor.sequence,
    };
    assert!(store.library_changes(Some(&foreign)).unwrap().reset);
}

#[test]
fn current_read_model_omits_archive_bodies_but_preserves_counts_and_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let saved = capture(&store);
    store
        .edit_memory(&EditRequest {
            request_id: Uuid::new_v4().to_string(),
            memory_id: saved.memory_id.clone(),
            expected_version: saved.version_id,
            title: "new title".into(),
            body: "new body".into(),
        })
        .unwrap();
    let key = RecordKey {
        kind: "memory".into(),
        id: saved.memory_id,
    };
    let current = store.library_detail_view(&key, false).unwrap();
    let archive = store.library_detail(&key).unwrap();
    assert_eq!(current.body, "new body");
    assert_eq!(
        current.current.unwrap().capture_ids,
        archive.current.unwrap().capture_ids
    );
    assert!(current.history.is_empty());
    assert!(current.sources.is_empty());
    assert_eq!(current.history_count, archive.history.len());
    assert_eq!(current.source_count, archive.sources.len());
    assert_eq!(current.history_count, 2);
}
