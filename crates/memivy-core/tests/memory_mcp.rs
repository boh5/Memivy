use memivy_core::memory::*;
use uuid::Uuid;
fn id() -> String {
    Uuid::new_v4().to_string()
}
fn request(text: &str) -> CaptureRequest {
    CaptureRequest {
        request_id: id(),
        text: text.into(),
        origin: Origin::Agent {
            app: "Synthetic Agent".into(),
            project: Some("项目 A".into()),
            uri: Some("https://example.test/session".into()),
        },
    }
}
fn query(text: &str) -> McpSearchQuery {
    McpSearchQuery {
        queries: vec![MemoryQuery::text(text)],
        ..Default::default()
    }
}
fn setup() -> (tempfile::TempDir, MemoryStore) {
    let dir = tempfile::tempdir().unwrap();
    let s = MemoryStore::open(dir.path()).unwrap();
    (dir, s)
}
#[test]
fn off_and_corrupt_configuration_fail_closed_with_no_capture() {
    let (dir, s) = setup();
    assert!(s.mcp_enabled());
    s.set_mcp_enabled(false).unwrap();
    assert!(!MemoryStore::open(dir.path()).unwrap().mcp_enabled());
    let req = request("关闭测试");
    assert_eq!(s.mcp_capture(&req).unwrap_err(), DataError::McpDisabled);
    assert_eq!(
        s.mcp_search(&query("关闭")).unwrap_err(),
        DataError::McpDisabled
    );
    s.set_mcp_enabled(true).unwrap();
    std::fs::write(dir.path().join("mcp.json"), "{broken").unwrap();
    assert_eq!(s.mcp_capture(&req).unwrap_err(), DataError::McpDisabled);
    assert!(
        s.library(&LibraryQuery::default())
            .unwrap()
            .items
            .is_empty()
    );
}
#[test]
fn retries_preserve_exact_text_source_and_immediate_searchability() {
    let (_dir, s) = setup();
    s.set_mcp_enabled(true).unwrap();
    let mut req = request("  请记住：原话\n不能改写！  ");
    let receipt = s.mcp_capture(&req).unwrap();
    assert_eq!(s.mcp_capture(&req).unwrap().capture_id, receipt.capture_id);
    assert_eq!(s.capture_by_id(&receipt.capture_id).unwrap().text, req.text);
    req.text.push('！');
    assert_eq!(s.mcp_capture(&req).unwrap_err(), DataError::RequestConflict);
    // With no app or model running, capture/search remain fully usable.
    assert_eq!(s.mcp_search(&query("不能改写")).unwrap().items.len(), 1);
    s.set_mcp_enabled(false).unwrap();
    assert_eq!(
        s.mcp_search(&query("不能改写")).unwrap_err(),
        DataError::McpDisabled
    );
}
#[test]
fn bounded_search_filters_before_limit_and_excludes_transient_and_deleted_data() {
    let (_dir, s) = setup();
    s.set_mcp_enabled(true).unwrap();
    for n in 0..12 {
        s.mcp_capture(&request(&format!("合成边界记忆 {n}")))
            .unwrap();
    }
    let mut other = request("独有过滤 合成边界记忆");
    other.origin = Origin::User {
        app: "Memivy".into(),
        project: Some("项目 B".into()),
        uri: None,
    };
    s.capture(&other).unwrap();
    assert_eq!(s.mcp_search(&query("合成边界记忆")).unwrap().items.len(), 5);
    let mut q = query("合成边界记忆");
    q.limit = Some(8);
    let found = s.mcp_search(&q).unwrap();
    assert_eq!(found.items.len(), 8);
    assert!(found.has_more);
    q.origin = Some("user".into());
    q.project = Some("项目 B".into());
    q.limit = Some(1);
    assert_eq!(s.mcp_search(&q).unwrap().items.len(), 1);
    q.limit = Some(9);
    assert_eq!(s.mcp_search(&q).unwrap_err(), DataError::Invalid);
    assert_eq!(
        s.mcp_search(&query(" \n ")).unwrap_err(),
        DataError::Invalid
    );
    let c = id();
    s.create_conversation(&c, "私有草稿秘密").unwrap();
    s.save_workspace_draft(&WorkspaceDraft {
        context_export: None,
        destination: None,
        key: "input".into(),
        request_id: id(),
        title: "私有草稿秘密".into(),
        body: "私有草稿秘密".into(),
        expected_version: None,
        origin: None,
        context: vec![],
    })
    .unwrap();
    let deleted = s.mcp_capture(&request("私有删除秘密")).unwrap();
    s.trash_memory(
        &deleted.memory_id,
        &s.memory(&deleted.memory_id).unwrap().current.id,
    )
    .unwrap();
    assert!(s.mcp_search(&query("私有")).unwrap().items.is_empty());
}
#[test]
fn snippets_reference_the_used_version_or_original_and_omit_old_bodies() {
    let (_dir, s) = setup();
    s.set_mcp_enabled(true).unwrap();
    let raw = s.mcp_capture(&request("原始证据 蜂蜜柚子茶")).unwrap();
    let receipt = s
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: raw.memory_id.clone(),
            expected_version: s.memory(&raw.memory_id).unwrap().current.id,
            title: "当前标题".into(),
            body: "当前正文 绿茶".into(),
        })
        .unwrap();
    let current = s.mcp_search(&query("绿茶")).unwrap();
    assert!(
        matches!(&current.items[0].source, SourceRef::Version(v) if Some(v)==receipt.after_version.as_ref())
    );
    let source = s.mcp_search(&query("蜂蜜柚子茶")).unwrap();
    assert!(source.items.is_empty());
    assert!(
        s.capture_by_id(&raw.capture_id)
            .unwrap()
            .text
            .contains("蜂蜜柚子茶")
    );
    let updated = s
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: receipt.memory_id.clone().unwrap(),
            expected_version: receipt.after_version.clone().unwrap(),
            title: "New version".into(),
            body: "新版正文 红茶".into(),
        })
        .unwrap();
    assert!(s.mcp_search(&query("绿茶")).unwrap().items.is_empty());
    s.trash_memory(
        updated.memory_id.as_ref().unwrap(),
        updated.after_version.as_ref().unwrap(),
    )
    .unwrap();
    assert!(s.mcp_search(&query("蜂蜜柚子茶")).unwrap().items.is_empty());
}
#[test]
fn mixed_source_search_reports_imported_authorship_beyond_the_origin_sample_limit() {
    let (_dir, store) = setup();
    let mut target_request = request("MixedSourceToken 引文：我已经完成演示。这是来源材料。");
    target_request.origin = Origin::Agent {
        app: "Agent 0".into(),
        project: None,
        uri: None,
    };
    let target = store.capture(&target_request).unwrap();
    let mut source_ids = vec![];
    for index in 1..5 {
        let source = store
            .capture(&CaptureRequest {
                request_id: id(),
                text: format!("Other original {index}"),
                origin: Origin::Agent {
                    app: format!("Agent {index}"),
                    project: None,
                    uri: None,
                },
            })
            .unwrap();
        source_ids.push(source.capture_id);
    }
    let imported = store
        .import_files(&ImportRequest {
            request_id: id(),
            files: vec![ImportFile {
                file_name: "quotation.md".into(),
                text: "我已经完成演示。这是外部作者的原话。".into(),
            }],
        })
        .unwrap();
    let imported_memory = store.memory(&imported.items[0].memory_id).unwrap();
    source_ids.push(imported_memory.current.capture_ids[0].clone());
    let db = rusqlite::Connection::open(store.database_path()).unwrap();
    // Model the source links on a version that combines several inputs. The
    // imported origin sorts after five distinct Agent origins in search output.
    for capture_id in source_ids {
        db.execute(
            "INSERT INTO version_captures(version_id,capture_id) VALUES(?1,?2)",
            rusqlite::params![target.version_id, capture_id],
        )
        .unwrap();
    }
    let regular = store
        .search(&SearchRequest::text("MixedSourceToken", 5))
        .unwrap();
    assert_eq!(regular.items[0].origins.len(), 4);
    assert!(
        regular.items[0]
            .origins
            .iter()
            .all(|origin| matches!(origin, Origin::Agent { .. }))
    );
    let mut filtered = query("MixedSourceToken");
    filtered.origin = Some("imported".into());
    let result = store.mcp_search(&filtered).unwrap();
    assert_eq!(result.items.len(), 1);
    let hit = &result.items[0];
    assert_eq!(hit.record.id, target.memory_id);
    assert!(matches!(&hit.source, SourceRef::Version(version) if version == &target.version_id));
    assert!(matches!(&hit.origin, Some(Origin::Agent { .. })));
    assert_eq!(hit.total_sources, 6);
    assert!(hit.more_sources_available);
    assert!(hit.contains_imported_material);
    assert!(result.notice.contains("unknown authorship"));
    assert!(result.notice.contains("not automatically the user's"));
    let plain = store.mcp_search(&query("Other original 1")).unwrap();
    assert_eq!(plain.items[0].total_sources, 1);
    assert!(!plain.items[0].more_sources_available);
    assert!(!plain.items[0].contains_imported_material);
}
#[test]
fn watcher_detects_external_commits_without_a_model() {
    let (dir, s) = setup();
    let mut watcher = s.change_watcher().unwrap();
    let external = MemoryStore::open(dir.path()).unwrap();
    assert!(!watcher.changed().unwrap());
    external.set_mcp_enabled(true).unwrap();
    external
        .mcp_capture(&request("来自另一个进程的原话"))
        .unwrap();
    assert!(watcher.changed().unwrap());
    assert!(!watcher.changed().unwrap());
}
#[test]
fn draft_autosave_and_index_work_do_not_refresh_the_library() {
    let (dir, s) = setup();
    let conversation = id();
    s.create_conversation(&conversation, "Draft test").unwrap();
    let memory = s.capture(&request("编辑草稿的原始内容")).unwrap();
    let mut watcher = s.change_watcher().unwrap();
    let db = rusqlite::Connection::open(dir.path().join("memivy.db")).unwrap();
    for key in [
        "input".to_string(),
        "quick_input".into(),
        format!("discussion:{conversation}"),
        format!("memory:{}", memory.memory_id),
    ] {
        let mut previous = None;
        for i in 0..3 {
            let draft = WorkspaceDraft {
                context_export: None,
                key: key.clone(),
                request_id: id(),
                title: String::new(),
                body: format!("中文草稿 {i}"),
                expected_version: None,
                origin: None,
                context: vec![],
                destination: None,
            };
            assert!(
                s.compare_workspace_draft(&draft, previous.as_deref())
                    .unwrap()
            );
            previous = Some(draft.request_id);
            assert!(
                !watcher.changed().unwrap(),
                "autosave refreshed library: {key}"
            );
        }
        assert!(
            s.consume_workspace_draft(&key, previous.as_deref().unwrap())
                .unwrap()
        );
        assert!(!watcher.changed().unwrap());
    }
    db.execute("DELETE FROM workspace_drafts", []).unwrap();
    assert!(!watcher.changed().unwrap());
    db.execute(
        "INSERT INTO embedding_index_meta VALUES(1,'synthetic','test','building',0,0)",
        [],
    )
    .unwrap();
    db.execute("UPDATE embedding_index_meta SET cursor=1,state='ready'", [])
        .unwrap();
    assert!(!watcher.changed().unwrap());
    s.rebuild_search_index().unwrap();
    assert!(!watcher.changed().unwrap());
    let saved = s.capture(&request("真实保存仍须刷新")).unwrap();
    assert!(watcher.changed().unwrap());
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    db.execute(
        "UPDATE memories SET updated_at=updated_at+1 WHERE id=?",
        [&saved.memory_id],
    )
    .unwrap();
    assert!(!watcher.changed().unwrap(), "uncommitted changes leaked");
    db.execute_batch("ROLLBACK").unwrap();
    assert!(!watcher.changed().unwrap());
    db.execute(
        "INSERT INTO record_pins VALUES('memory',?1,1)",
        [&saved.memory_id],
    )
    .unwrap();
    assert!(watcher.changed().unwrap());
    db.execute("DELETE FROM record_pins", []).unwrap();
    assert!(watcher.changed().unwrap());
}
#[test]
fn switch_is_serialized_with_inflight_readers_and_never_fakes_success() {
    let (dir, s) = setup();
    s.set_mcp_enabled(true).unwrap();
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.path().join("mcp.lock"))
        .unwrap();
    file.lock_shared().unwrap();
    assert_eq!(s.set_mcp_enabled(false).unwrap_err(), DataError::Busy);
    assert!(s.mcp_enabled());
    drop(file);
    s.set_mcp_enabled(false).unwrap();
    assert!(!s.mcp_enabled());
}
