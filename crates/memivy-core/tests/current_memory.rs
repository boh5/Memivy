use memivy_core::memory::*;
use uuid::Uuid;

fn id() -> String {
    Uuid::new_v4().to_string()
}
fn request(text: &str) -> CaptureRequest {
    CaptureRequest {
        request_id: id(),
        text: text.into(),
        origin: Origin::User {
            app: "test".into(),
            project: None,
            uri: None,
        },
    }
}
fn proposal(action: &str, text: &str) -> OrganizationProposal {
    OrganizationProposal {
        action: action.into(),
        target: String::new(),
        title: "整理后".into(),
        addition: text.into(),
        changes: vec![],
        keywords: vec![],
        reason: "独立事项".into(),
    }
}

#[test]
fn capture_is_immediately_one_memory_and_archive_is_not_searchable() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let request = request("报价旧值 100 EUR");
    let saved = store.capture(&request).unwrap();
    let replay = store.capture(&request).unwrap();
    assert_eq!(saved.memory_id, replay.memory_id);
    assert_eq!(saved.capture_id, replay.capture_id);
    assert_eq!(
        store.library(&LibraryQuery::default()).unwrap().items.len(),
        1
    );
    let hits = store.search(&SearchRequest::text("100", 8)).unwrap();
    assert_eq!(hits.items[0].memory_id, saved.memory_id);
    store
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: saved.memory_id.clone(),
            expected_version: saved.version_id,
            title: "当前报价".into(),
            body: "当前报价 200 EUR".into(),
        })
        .unwrap();
    assert!(
        store
            .search(&SearchRequest::text("100", 8))
            .unwrap()
            .items
            .is_empty()
    );
    assert!(
        store
            .search(&SearchRequest::text("旧", 8))
            .unwrap()
            .items
            .is_empty()
    );
    assert_eq!(
        store.capture_by_id(&saved.capture_id).unwrap().text,
        request.text
    );
    store.rebuild_search_index().unwrap();
    assert!(
        store
            .search(&SearchRequest::text("100", 8))
            .unwrap()
            .items
            .is_empty()
    );
    assert_eq!(
        store
            .search(&SearchRequest::text("200", 8))
            .unwrap()
            .items
            .len(),
        1
    );
    store.check_integrity().unwrap();
}

#[test]
fn automatic_organization_keeps_identity_and_yields_to_a_draft() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let saved = store.capture(&request("独立事项完整的细节")).unwrap();
    let task = store.claim_organization().unwrap().unwrap();
    store
        .save_workspace_draft(&WorkspaceDraft {
            conclusion: None,
            key: format!("memory:{}", saved.memory_id),
            request_id: id(),
            title: "用户标题".into(),
            body: "用户正在修改".into(),
            expected_version: Some(saved.version_id.clone()),
            origin: None,
            context: vec![],
        })
        .unwrap();
    assert!(matches!(
        store.apply_organization(&task, &proposal("keep", "整理好的细节")),
        Err(DataError::Conflict)
    ));
    assert_eq!(
        store.memory(&saved.memory_id).unwrap().current.id,
        saved.version_id
    );
    store
        .delete_workspace_draft(&format!("memory:{}", saved.memory_id))
        .unwrap();
    let receipt = store
        .apply_organization(&task, &proposal("keep", "整理好的细节"))
        .unwrap();
    assert_eq!(receipt.memory_id.as_deref(), Some(saved.memory_id.as_str()));
    assert_eq!(
        store.library(&LibraryQuery::default()).unwrap().items.len(),
        1
    );
}

#[test]
fn merge_is_atomic_and_purge_removes_the_hidden_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let target = store.capture(&request("主记忆")).unwrap();
    let target_task = store.claim_organization().unwrap().unwrap();
    store
        .apply_organization(&target_task, &proposal("keep", "主记忆"))
        .unwrap();
    let input = store.capture(&request("合并输入独有机密")).unwrap();
    let mut task = store.claim_organization().unwrap().unwrap();
    task.candidates
        .push(store.memory(&target.memory_id).unwrap().current);
    let mut p = proposal("merge", "合并输入独有机密");
    p.target = "M1".into();
    p.title.clear();
    let receipt = store.apply_organization(&task, &p).unwrap();
    assert_eq!(receipt.action, "merge");
    assert_eq!(
        store.library(&LibraryQuery::default()).unwrap().items.len(),
        1
    );
    store
        .trash_memory(&target.memory_id, receipt.after_version.as_deref().unwrap())
        .unwrap();
    store.purge_memory(&target.memory_id).unwrap();
    let db = rusqlite::Connection::open(store.database_path()).unwrap();
    let hidden: i64 = db
        .query_row(
            "SELECT count(*) FROM memory_versions WHERE memory_id=? AND body IS NOT NULL",
            [&input.memory_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(hidden, 0);
    store.check_integrity().unwrap();
}

#[test]
fn conclusion_conflict_preserves_review_draft_without_creating_an_archive() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let target = store.capture(&request("目标原正文")).unwrap();
    let topic = id();
    store.create_conversation(&topic, "讨论").unwrap();
    let turn = store.start_turn(&id(), &topic, "问题", &[]).unwrap();
    store.finish_turn(&turn.id, "回答", &[]).unwrap();
    let destination = Destination::Existing {
        memory_id: target.memory_id.clone(),
        expected_version: target.version_id.clone(),
    };
    let draft = WorkspaceDraft {
        key: format!("conclusion:{}", turn.assistant.id),
        request_id: id(),
        title: "审核标题".into(),
        body: "  审核结论\n原样保留 🙂  ".into(),
        expected_version: None,
        origin: None,
        context: vec![],
        conclusion: Some(ConclusionDraft {
            destination: destination.clone(),
            merged_body: Some("完整融合稿\n含原文和新结论".into()),
        }),
    };
    assert!(store.compare_workspace_draft(&draft, None).unwrap());
    store
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: target.memory_id.clone(),
            expected_version: target.version_id,
            title: "其他编辑".into(),
            body: "目标已改动".into(),
        })
        .unwrap();
    let save = ConclusionRequest {
        request_id: draft.request_id.clone(),
        message_id: turn.assistant.id,
        destination: destination.clone(),
        title: draft.title.clone(),
        text: draft.body.clone(),
    };
    assert_eq!(
        store
            .save_reviewed_conclusion(
                &save,
                draft.conclusion.as_ref().unwrap().merged_body.as_deref()
            )
            .unwrap()
            .status,
        "needs_review"
    );
    drop(store);
    let store = MemoryStore::open(dir.path()).unwrap();
    let recovered = store.workspace_draft(&draft.key).unwrap().unwrap();
    assert_eq!(recovered.body, draft.body);
    assert_eq!(recovered.title, draft.title);
    let review = recovered.conclusion.unwrap();
    assert_eq!(review.destination, destination);
    assert_eq!(review.merged_body, draft.conclusion.unwrap().merged_body);
    assert_eq!(
        store.memory(&target.memory_id).unwrap().current.body,
        "目标已改动"
    );
    let db = rusqlite::Connection::open(store.database_path()).unwrap();
    let archives: i64 = db
        .query_row("SELECT count(*) FROM captures", [], |r| r.get(0))
        .unwrap();
    assert_eq!(archives, 1);
    assert_eq!(
        store.library(&LibraryQuery::default()).unwrap().items.len(),
        1
    );
    assert!(
        store
            .search(&SearchRequest::text("审核结论", 8))
            .unwrap()
            .items
            .is_empty()
    );
    store.check_integrity().unwrap();
}

#[test]
fn confirmed_conclusion_consumes_its_draft_atomically_and_replays_once() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let target = store.capture(&request("原正文")).unwrap();
    let topic = id();
    store.create_conversation(&topic, "讨论").unwrap();
    let turn = store.start_turn(&id(), &topic, "问题", &[]).unwrap();
    store.finish_turn(&turn.id, "回答", &[]).unwrap();
    let save = ConclusionRequest {
        request_id: id(),
        message_id: turn.assistant.id,
        destination: Destination::Existing {
            memory_id: target.memory_id.clone(),
            expected_version: target.version_id,
        },
        title: "标题".into(),
        text: "确认的结论".into(),
    };
    let mut draft = WorkspaceDraft {
        key: format!("conclusion:{}", save.message_id),
        request_id: save.request_id.clone(),
        title: save.title.clone(),
        body: save.text.clone(),
        expected_version: None,
        origin: None,
        context: vec![],
        conclusion: Some(ConclusionDraft {
            destination: save.destination.clone(),
            merged_body: None,
        }),
    };
    store.save_workspace_draft(&draft).unwrap();
    let saved = store.save_conclusion(&save).unwrap();
    assert_eq!(saved.status, "applied");
    // Simulate process loss immediately after commit, before the UI receives it.
    drop(store);
    let store = MemoryStore::open(dir.path()).unwrap();
    assert!(store.workspace_draft(&draft.key).unwrap().is_none());
    draft.request_id = id();
    draft.body = "另一窗口的新草稿".into();
    store.save_workspace_draft(&draft).unwrap();
    let replay = store.save_conclusion(&save).unwrap();
    assert_eq!(replay.after_version, saved.after_version);
    assert_eq!(
        store.memory(&target.memory_id).unwrap().current.body,
        "原正文\n\n确认的结论"
    );
    assert_eq!(
        store.workspace_draft(&draft.key).unwrap().unwrap().body,
        draft.body
    );
    store.check_integrity().unwrap();
}

#[test]
fn archive_restore_requires_own_source_and_creates_a_reversible_current_version() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let saved = store.capture(&request("  初始报价 100 EUR\n ")).unwrap();
    let edited = store
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: saved.memory_id.clone(),
            expected_version: saved.version_id.clone(),
            title: "最新报价".into(),
            body: "新报价 200 EUR".into(),
        })
        .unwrap();
    let foreign = store.capture(&request("其他输入不能混入恢复")).unwrap();
    let expected = edited.after_version.as_ref().unwrap();
    assert_eq!(
        store
            .restore_archive(&id(), &saved.memory_id, expected, &foreign.capture_id)
            .unwrap_err(),
        DataError::Invalid
    );
    let request = id();
    let restored = store
        .restore_archive(&request, &saved.memory_id, expected, &saved.capture_id)
        .unwrap();
    assert_eq!(
        store
            .restore_archive(&request, &saved.memory_id, expected, &saved.capture_id)
            .unwrap(),
        restored
    );
    assert_eq!(
        store.memory(&saved.memory_id).unwrap().current.body,
        "  初始报价 100 EUR\n "
    );
    assert_eq!(
        store.memory(&saved.memory_id).unwrap().current.title,
        "最新报价"
    );
    assert_eq!(
        store
            .search(&SearchRequest::text("100", 8))
            .unwrap()
            .items
            .len(),
        1
    );
    assert!(
        store
            .search(&SearchRequest::text("200", 8))
            .unwrap()
            .items
            .is_empty()
    );
    store.undo(&id(), &restored.request_id).unwrap();
    assert_eq!(
        store.memory(&saved.memory_id).unwrap().current.body,
        "新报价 200 EUR"
    );
    assert_eq!(
        store.capture_by_id(&saved.capture_id).unwrap().text,
        "  初始报价 100 EUR\n "
    );
    assert!(
        store
            .search(&SearchRequest::text("100", 8))
            .unwrap()
            .items
            .is_empty()
    );
    store.check_integrity().unwrap();
}

#[test]
fn lone_edited_pending_memory_is_persistently_paused() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let saved = store.capture(&request("唯一待整理输入")).unwrap();
    store
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: saved.memory_id.clone(),
            expected_version: saved.version_id,
            title: "手工标题".into(),
            body: "已经手工编辑".into(),
        })
        .unwrap();
    assert!(store.claim_organization().unwrap().is_none());
    let reopened = MemoryStore::open(dir.path()).unwrap();
    let jobs = reopened
        .organization_jobs(&RecordKey {
            kind: "memory".into(),
            id: saved.memory_id,
        })
        .unwrap();
    assert_eq!(jobs[0].status, "paused");
    assert!(!jobs[0].can_retry);
    assert!(reopened.claim_organization().unwrap().is_none());
}

#[test]
fn merge_yields_to_an_unsaved_target_draft_without_changing_either_memory() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let target = store.capture(&request("目标当前正文")).unwrap();
    let target_task = store.claim_organization().unwrap().unwrap();
    store
        .apply_organization(&target_task, &proposal("keep", "目标当前正文"))
        .unwrap();
    let input = store.capture(&request("新补充内容")).unwrap();
    let mut task = store.claim_organization().unwrap().unwrap();
    let before = store.memory(&target.memory_id).unwrap().current;
    task.candidates.push(before.clone());
    let draft = WorkspaceDraft {
        key: format!("memory:{}", target.memory_id),
        request_id: id(),
        title: before.title.clone(),
        body: "用户还没保存的编辑".into(),
        expected_version: Some(before.id.clone()),
        origin: None,
        context: vec![],
        conclusion: None,
    };
    store.save_workspace_draft(&draft).unwrap();
    let mut p = proposal("merge", "新补充内容");
    p.target = "M1".into();
    p.title.clear();
    assert_eq!(
        store.apply_organization(&task, &p).unwrap_err(),
        DataError::Conflict
    );
    assert_eq!(
        store.memory(&target.memory_id).unwrap().current.id,
        before.id
    );
    assert_eq!(
        store.memory(&input.memory_id).unwrap().current.id,
        input.version_id
    );
    assert_eq!(
        store.workspace_draft(&draft.key).unwrap().unwrap().body,
        draft.body
    );
    assert_eq!(
        store.library(&LibraryQuery::default()).unwrap().items.len(),
        2
    );
}

#[test]
fn deleting_a_discussion_removes_its_review_drafts_but_preserves_saved_memories() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let topic = id();
    store.create_conversation(&topic, "讨论").unwrap();
    let turn = store.start_turn(&id(), &topic, "问题", &[]).unwrap();
    store.finish_turn(&turn.id, "回答", &[]).unwrap();
    let saved = store
        .save_conclusion(&ConclusionRequest {
            request_id: id(),
            message_id: turn.assistant.id.clone(),
            destination: Destination::New,
            title: "已确认".into(),
            text: "已保存的内容".into(),
        })
        .unwrap();
    let draft = WorkspaceDraft {
        key: format!("conclusion:{}", turn.assistant.id),
        request_id: id(),
        title: "未保存".into(),
        body: "待审核结论".into(),
        expected_version: None,
        origin: None,
        context: vec![],
        conclusion: Some(ConclusionDraft {
            destination: Destination::New,
            merged_body: None,
        }),
    };
    store.save_workspace_draft(&draft).unwrap();
    store.delete_conversation(&topic).unwrap();
    assert!(store.workspace_draft(&draft.key).unwrap().is_none());
    assert_eq!(
        store.save_workspace_draft(&draft).unwrap_err(),
        DataError::Unavailable
    );
    assert_eq!(
        store
            .memory(saved.memory_id.as_ref().unwrap())
            .unwrap()
            .current
            .body,
        "已保存的内容"
    );
    store.check_integrity().unwrap();
}

#[test]
fn organization_off_blocks_new_effects_but_preserves_receipt_replay() {
    use memivy_core::models::Registry;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    store.capture(&request("已整理事项")).unwrap();
    let first = store.claim_organization().unwrap().unwrap();
    let proposal = proposal("keep", "整理结果");
    let receipt = store.apply_organization(&first, &proposal).unwrap();
    let saved = store.capture(&request("新事项原文")).unwrap();
    let pending = store.claim_organization().unwrap().unwrap();
    let mut registry = Registry {
        auto_organize: false,
        ..Registry::default()
    };
    registry.save(dir.path(), "initial").unwrap();
    assert_eq!(
        store.apply_organization(&first, &proposal).unwrap(),
        receipt
    );
    assert!(matches!(
        store.apply_organization(&pending, &proposal),
        Err(DataError::Conflict)
    ));
    assert_eq!(
        store.memory(&saved.memory_id).unwrap().current.body,
        "新事项原文"
    );
}
