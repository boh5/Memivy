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
