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
fn proposal(task: &OrganizationTask, text: &str) -> MemoryWriteArgs {
    MemoryWriteArgs {
        destination: Destination::New,
        title: "Organized".into(),
        parts: vec![MemoryWritePart {
            text: text.into(),
            sources: vec![MemorySourceQuote {
                source_id: task.capture_id.clone(),
                quote: task.memory.body.trim().chars().take(512).collect(),
            }],
        }],
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
            destination: None,
            key: format!("memory:{}", saved.memory_id),
            request_id: id(),
            title: "User title".into(),
            body: "用户正在修改".into(),
            expected_version: Some(saved.version_id.clone()),
            origin: None,
            context: vec![],
        })
        .unwrap();
    assert!(matches!(
        store.apply_organization(&task, &proposal(&task, "整理好的细节")),
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
        .apply_organization(&task, &proposal(&task, "整理好的细节"))
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
        .apply_organization(&target_task, &proposal(&target_task, "主记忆"))
        .unwrap();
    let input = store.capture(&request("合并输入独有机密")).unwrap();
    let task = store.claim_organization().unwrap().unwrap();
    let target_version = store.memory(&target.memory_id).unwrap().current;
    let mut p = proposal(&task, "主记忆\n\n合并输入独有机密");
    p.destination = Destination::Existing {
        memory_id: target.memory_id.clone(),
        expected_version: target_version.id,
    };
    p.title = target_version.title;
    p.parts = vec![
        MemoryWritePart {
            text: "主记忆\n\n".into(),
            sources: vec![],
        },
        proposal(&task, "合并输入独有机密").parts.remove(0),
    ];
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
fn manual_save_conflict_preserves_draft_without_creating_an_archive() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let target = store.capture(&request("目标原正文")).unwrap();
    let topic = id();
    store.create_conversation(&topic, "Discussion").unwrap();
    let run = store
        .begin_agent_input(&id(), &id(), &topic, "问题", &[], None)
        .unwrap();
    store
        .append_agent_text(&run.input_id, &run.attempt_id, "回答")
        .unwrap();
    store
        .finish_agent_input(&run.input_id, &run.attempt_id, &[])
        .unwrap();
    let destination = Destination::Existing {
        memory_id: target.memory_id.clone(),
        expected_version: target.version_id.clone(),
    };
    let draft = WorkspaceDraft {
        key: format!("save:{}", run.assistant_message_id),
        request_id: id(),
        title: "Manual title".into(),
        body: "  用户选择的文字\n原样保留 🙂  ".into(),
        expected_version: None,
        origin: None,
        context: vec![],
        destination: Some(destination.clone()),
    };
    store.compare_workspace_draft(&draft, None).unwrap();
    store
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: target.memory_id.clone(),
            expected_version: target.version_id,
            title: "Other edit".into(),
            body: "目标已改动".into(),
        })
        .unwrap();
    assert_eq!(
        store
            .save_agent_text(
                &draft.request_id,
                &run.input_id,
                &draft.body,
                &draft.title,
                &destination
            )
            .unwrap_err(),
        DataError::Conflict
    );
    drop(store);
    let store = MemoryStore::open(dir.path()).unwrap();
    let recovered = store.workspace_draft(&draft.key).unwrap().unwrap();
    assert_eq!(recovered.body, draft.body);
    assert_eq!(recovered.title, draft.title);
    assert_eq!(recovered.destination, Some(destination));
    assert_eq!(
        store.memory(&target.memory_id).unwrap().current.body,
        "目标已改动"
    );
    assert_eq!(
        rusqlite::Connection::open(store.database_path())
            .unwrap()
            .query_row("SELECT count(*) FROM captures", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        store.library(&LibraryQuery::default()).unwrap().items.len(),
        1
    );
    assert!(
        store
            .search(&SearchRequest::text("用户选择", 8))
            .unwrap()
            .items
            .is_empty()
    );
    store.check_integrity().unwrap();
}

#[test]
fn manual_save_consumes_its_draft_atomically_and_replay_preserves_a_newer_draft() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let target = store.capture(&request("原正文")).unwrap();
    let topic = id();
    store.create_conversation(&topic, "Discussion").unwrap();
    let run = store
        .begin_agent_input(&id(), &id(), &topic, "问题", &[], None)
        .unwrap();
    store
        .append_agent_text(&run.input_id, &run.attempt_id, "回答")
        .unwrap();
    store
        .finish_agent_input(&run.input_id, &run.attempt_id, &[])
        .unwrap();
    let request = id();
    let destination = Destination::Existing {
        memory_id: target.memory_id.clone(),
        expected_version: target.version_id,
    };
    let text = "确认的文字";
    let title = "Title";
    let mut draft = WorkspaceDraft {
        key: format!("save:{}", run.assistant_message_id),
        request_id: request.clone(),
        title: title.into(),
        body: text.into(),
        expected_version: None,
        origin: None,
        context: vec![],
        destination: Some(destination.clone()),
    };
    store.save_workspace_draft(&draft).unwrap();
    let saved = store
        .save_agent_text(&request, &run.input_id, text, title, &destination)
        .unwrap();
    drop(store);
    let store = MemoryStore::open(dir.path()).unwrap();
    assert!(store.workspace_draft(&draft.key).unwrap().is_none());
    draft.request_id = id();
    draft.body = "另一窗口的新草稿".into();
    store.save_workspace_draft(&draft).unwrap();
    let replay = store
        .save_agent_text(&request, &run.input_id, text, title, &destination)
        .unwrap();
    assert_eq!(replay.after_version, saved.after_version);
    assert_eq!(
        store.memory(&target.memory_id).unwrap().current.body,
        "原正文\n\n确认的文字"
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
            title: "Manual title".into(),
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
        .apply_organization(&target_task, &proposal(&target_task, "目标当前正文"))
        .unwrap();
    let input = store.capture(&request("新补充内容")).unwrap();
    let task = store.claim_organization().unwrap().unwrap();
    let before = store.memory(&target.memory_id).unwrap().current;
    let draft = WorkspaceDraft {
        key: format!("memory:{}", target.memory_id),
        request_id: id(),
        title: before.title.clone(),
        body: "用户还没保存的编辑".into(),
        expected_version: Some(before.id.clone()),
        origin: None,
        context: vec![],
        destination: None,
    };
    store.save_workspace_draft(&draft).unwrap();
    let mut p = proposal(&task, "目标当前正文\n\n新补充内容");
    p.destination = Destination::Existing {
        memory_id: target.memory_id.clone(),
        expected_version: before.id.clone(),
    };
    p.title = before.title.clone();
    p.parts = vec![
        MemoryWritePart {
            text: "目标当前正文\n\n".into(),
            sources: vec![],
        },
        proposal(&task, "新补充内容").parts.remove(0),
    ];
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
fn deleting_a_discussion_removes_its_unsaved_selection_but_preserves_saved_memories() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let topic = id();
    store.create_conversation(&topic, "Discussion").unwrap();
    let run = store
        .begin_agent_input(&id(), &id(), &topic, "问题", &[], None)
        .unwrap();
    store
        .append_agent_text(&run.input_id, &run.attempt_id, "回答")
        .unwrap();
    store
        .finish_agent_input(&run.input_id, &run.attempt_id, &[])
        .unwrap();
    let saved = store
        .save_agent_text(
            &id(),
            &run.input_id,
            "已保存的内容",
            "手动保存",
            &Destination::New,
        )
        .unwrap();
    let draft = WorkspaceDraft {
        key: format!("save:{}", run.assistant_message_id),
        request_id: id(),
        title: "未保存".into(),
        body: "未发送文字".into(),
        expected_version: None,
        origin: None,
        context: vec![],
        destination: Some(Destination::New),
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
    use memivy_core::models::ModelSettings;
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    store.capture(&request("已整理事项")).unwrap();
    let first = store.claim_organization().unwrap().unwrap();
    let proposal = proposal(&first, "整理结果");
    let receipt = store.apply_organization(&first, &proposal).unwrap();
    let saved = store.capture(&request("新事项原文")).unwrap();
    let pending = store.claim_organization().unwrap().unwrap();
    let mut registry = ModelSettings {
        auto_organize: false,
        ..ModelSettings::default()
    };
    registry.save(dir.path(), "initial").unwrap();
    assert_eq!(
        store.apply_organization(&first, &proposal).unwrap(),
        receipt
    );
    assert!(matches!(
        store.apply_organization(
            &pending,
            &MemoryWriteArgs {
                parts: vec![MemoryWritePart {
                    text: "整理结果".into(),
                    sources: vec![MemorySourceQuote {
                        source_id: pending.capture_id.clone(),
                        quote: pending.memory.body.clone()
                    }],
                }],
                ..proposal.clone()
            }
        ),
        Err(DataError::Conflict)
    ));
    assert_eq!(
        store.memory(&saved.memory_id).unwrap().current.body,
        "新事项原文"
    );
}
