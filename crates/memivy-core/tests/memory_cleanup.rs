use memivy_core::memory::*;
use uuid::Uuid;
fn id() -> String {
    Uuid::new_v4().to_string()
}
fn setup() -> (tempfile::TempDir, MemoryStore, Receipt, RawCapture) {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let capture_request = id();
    let raw = store
        .capture(&CaptureRequest {
            request_id: capture_request.clone(),
            text: "9月9日可能上线，不承诺。预算 2500 元。".into(),
            origin: Origin::User {
                app: "test".into(),
                project: None,
                uri: None,
            },
        })
        .unwrap();
    let receipt = store.receipt(&capture_request).unwrap();
    let archive = store.capture_by_id(&raw.capture_id).unwrap();
    (dir, store, receipt, archive)
}
fn prepare(store: &MemoryStore, r: &Receipt) -> CleanupSnapshot {
    store
        .prepare_cleanup(
            r.memory_id.as_deref().unwrap(),
            r.after_version.as_deref().unwrap(),
        )
        .unwrap()
}
fn draft(snapshot: &CleanupSnapshot) -> WorkspaceDraft {
    WorkspaceDraft {
        destination: None,
        key: format!("memory:{}", snapshot.memory_id),
        request_id: id(),
        title: "草稿标题".into(),
        body: "未保存的草稿：也许推迟。".into(),
        expected_version: Some(snapshot.expected_version.clone()),
        origin: None,
        context: vec![],
    }
}
#[test]
fn reviewed_cleanup_preserves_sources_replays_and_undoes() {
    let (_dir, store, first, raw) = setup();
    let baseline = prepare(&store, &first);
    let source_draft = draft(&baseline);
    store.save_workspace_draft(&source_draft).unwrap();
    let snapshot = prepare(&store, &first);
    assert_eq!(snapshot.body, source_draft.body);
    let request = CleanupSave {
        request_id: id(),
        snapshot: snapshot.clone(),
        body: "## 时间\n\n也许推迟。".into(),
    };
    let receipt = store.save_cleanup(&request).unwrap();
    assert_eq!(store.save_cleanup(&request).unwrap(), receipt);
    assert!(store.workspace_draft(&source_draft.key).unwrap().is_none());
    let current = store.memory(&snapshot.memory_id).unwrap().current;
    assert_eq!(current.body, request.body);
    assert_eq!(current.title, source_draft.title);
    assert_eq!(current.reason, "cleanup");
    assert_eq!(current.capture_ids, vec![raw.id.clone()]);
    assert_eq!(store.capture_by_id(&raw.id).unwrap().text, raw.text);
    let restored = store.undo(&id(), &receipt.request_id).unwrap();
    assert_eq!(
        store
            .memory(restored.memory_id.as_deref().unwrap())
            .unwrap()
            .current
            .body,
        source_draft.body
    );
    assert_eq!(store.capture_by_id(&raw.id).unwrap().text, raw.text);
}
#[test]
fn changed_or_new_draft_and_version_conflicts_fail_closed() {
    let (_dir, store, first, _) = setup();
    let baseline = prepare(&store, &first);
    let request = CleanupSave {
        request_id: id(),
        snapshot: baseline.clone(),
        body: "候选".into(),
    };
    let mut d = draft(&baseline);
    store.save_workspace_draft(&d).unwrap();
    assert_eq!(
        store.save_cleanup(&request).unwrap_err(),
        DataError::Conflict
    );
    let snapshot = prepare(&store, &first);
    d.request_id = id();
    d.body = "另一个窗口的草稿".into();
    store.save_workspace_draft(&d).unwrap();
    assert_eq!(
        store
            .save_cleanup(&CleanupSave {
                request_id: id(),
                snapshot,
                body: "审核稿".into()
            })
            .unwrap_err(),
        DataError::Conflict
    );
    assert_eq!(store.workspace_draft(&d.key).unwrap().unwrap().body, d.body);
    let snapshot = prepare(&store, &first);
    store
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: baseline.memory_id,
            expected_version: baseline.expected_version,
            title: "New version".into(),
            body: "其他修改".into(),
        })
        .unwrap();
    assert_eq!(
        store
            .save_cleanup(&CleanupSave {
                request_id: id(),
                snapshot,
                body: "过期审核稿".into()
            })
            .unwrap_err(),
        DataError::Conflict
    );
}
#[test]
fn no_op_and_invalid_review_do_not_create_versions() {
    let (_dir, store, first, _) = setup();
    let snapshot = prepare(&store, &first);
    for body in [
        snapshot.body.clone(),
        " ".into(),
        "x".repeat(128 * 1024 + 1),
    ] {
        assert_eq!(
            store
                .save_cleanup(&CleanupSave {
                    request_id: id(),
                    snapshot: snapshot.clone(),
                    body
                })
                .unwrap_err(),
            DataError::Invalid
        );
    }
    assert_eq!(prepare(&store, &first), snapshot);
}

#[test]
fn failed_receipt_rolls_back_draft_snapshot_version_and_draft_consumption() {
    let (_dir, store, first, _) = setup();
    let baseline = prepare(&store, &first);
    let d = draft(&baseline);
    store.save_workspace_draft(&d).unwrap();
    let snapshot = prepare(&store, &first);
    let db = rusqlite::Connection::open(store.database_path()).unwrap();
    db.execute_batch("CREATE TRIGGER fail_cleanup_receipt BEFORE INSERT ON receipts BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
    assert_eq!(
        store
            .save_cleanup(&CleanupSave {
                request_id: id(),
                snapshot: snapshot.clone(),
                body: "候选结果".into()
            })
            .unwrap_err(),
        DataError::Database
    );
    assert_eq!(prepare(&store, &first), snapshot);
    assert_eq!(store.history(&snapshot.memory_id).unwrap().len(), 1);
    assert_eq!(store.workspace_draft(&d.key).unwrap().unwrap().body, d.body);
}

#[test]
fn reviewed_provenance_is_immutable_and_undo_refuses_later_edits() {
    let (_dir, store, first, _) = setup();
    let snapshot = prepare(&store, &first);
    let request = CleanupSave {
        request_id: id(),
        snapshot: snapshot.clone(),
        body: "整理后的正文".into(),
    };
    let receipt = store.save_cleanup(&request).unwrap();
    let db = rusqlite::Connection::open(store.database_path()).unwrap();
    assert!(
        db.execute(
            "UPDATE memory_versions SET review_kind=NULL WHERE id=?",
            [receipt.after_version.as_deref().unwrap()]
        )
        .is_err()
    );
    store
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: snapshot.memory_id.clone(),
            expected_version: receipt.after_version.clone().unwrap(),
            title: "New version".into(),
            body: "之后的修改".into(),
        })
        .unwrap();
    // A lost acknowledgement may be retried even after another successful edit.
    assert_eq!(store.save_cleanup(&request).unwrap(), receipt);
    assert_eq!(store.history(&snapshot.memory_id).unwrap().len(), 3);
    assert_eq!(
        store.undo(&id(), &receipt.request_id).unwrap_err(),
        DataError::Conflict
    );
    assert_eq!(
        store.memory(&snapshot.memory_id).unwrap().current.body,
        "之后的修改"
    );
}

#[test]
fn review_can_restore_saved_text_while_preserving_the_draft_in_history() {
    let (_dir, store, first, _) = setup();
    let baseline = prepare(&store, &first);
    let mut d = draft(&baseline);
    d.title = baseline.title.clone();
    store.save_workspace_draft(&d).unwrap();
    let snapshot = prepare(&store, &first);
    let request = CleanupSave {
        request_id: id(),
        snapshot,
        body: baseline.body.clone(),
    };
    let receipt = store.save_cleanup(&request).unwrap();
    assert_eq!(
        store.memory(&baseline.memory_id).unwrap().current.body,
        baseline.body
    );
    assert_eq!(store.history(&baseline.memory_id).unwrap().len(), 3);
    assert!(store.workspace_draft(&d.key).unwrap().is_none());
    store.undo(&id(), &receipt.request_id).unwrap();
    assert_eq!(
        store.memory(&baseline.memory_id).unwrap().current.body,
        d.body
    );
}
