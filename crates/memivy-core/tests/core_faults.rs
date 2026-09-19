use memivy_core::memory::*;
use uuid::Uuid;
fn id() -> String {
    Uuid::new_v4().to_string()
}
fn capture(s: &MemoryStore) -> RawCapture {
    let saved = s
        .capture(&CaptureRequest {
            request_id: id(),
            text: " \n故障中的原话 SQLite 中文\n ".into(),
            origin: Origin::User {
                app: "fixture".into(),
                project: None,
                uri: None,
            },
        })
        .unwrap();
    s.capture_by_id(&saved.capture_id).unwrap()
}
fn proposal(task: &OrganizationTask) -> MemoryWriteArgs {
    MemoryWriteArgs {
        destination: Destination::New,
        title: "故障恢复".into(),
        parts: vec![MemoryWritePart {
            text: "故障中的原话 SQLite 中文 回归".into(),
            sources: vec![MemorySourceQuote {
                source_id: task.capture_id.clone(),
                quote: "故障中的原话 SQLite 中文".into(),
            }],
        }],
    }
}
fn counts(s: &MemoryStore) -> Vec<i64> {
    let db = rusqlite::Connection::open(s.database_path()).unwrap();
    [
        "captures",
        "memories",
        "memory_versions",
        "receipts",
        "memory_keywords",
    ]
    .iter()
    .map(|t| {
        db.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))
            .unwrap()
    })
    .collect()
}
#[test]
fn organization_failure_after_receipt_and_index_updates_rolls_back_every_effect() {
    let temp = tempfile::tempdir().unwrap();
    let s = MemoryStore::open(temp.path()).unwrap();
    capture(&s);
    let task = s.claim_organization().unwrap().unwrap();
    let before = counts(&s);
    let db = rusqlite::Connection::open(s.database_path()).unwrap();
    db.execute_batch("CREATE TRIGGER inject_late_failure BEFORE UPDATE ON organization_jobs WHEN NEW.status='done' BEGIN SELECT RAISE(ABORT,'synthetic_api_key_never_export_47291'); END;").unwrap();
    let e = s.apply_organization(&task, &proposal(&task)).unwrap_err();
    assert_eq!(e, DataError::Database);
    assert!(!format!("{e:?} {e}").contains("synthetic_api_key_never_export_47291"));
    assert_eq!(counts(&s), before);
    assert_eq!(
        s.memory(&task.memory.memory_id).unwrap().current.id,
        task.memory.id
    );
    assert!(
        s.library(&LibraryQuery {
            query: "回归".into(),
            ..Default::default()
        })
        .unwrap()
        .items
        .is_empty()
    );
    db.execute_batch("DROP TRIGGER inject_late_failure")
        .unwrap();
    let p = proposal(&task);
    let r = s.apply_organization(&task, &p).unwrap();
    assert_eq!(s.apply_organization(&task, &p).unwrap(), r);
    assert_eq!(counts(&s), vec![1, 1, 2, 2, 0]);
    s.check_integrity().unwrap();
}
