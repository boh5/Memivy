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
        query: text.into(),
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
fn retries_preserve_exact_text_source_and_one_pending_job() {
    let (_dir, s) = setup();
    s.set_mcp_enabled(true).unwrap();
    let mut req = request("  请记住：原话\n不能改写！  ");
    let receipt = s.mcp_capture(&req).unwrap();
    assert_eq!(s.mcp_capture(&req).unwrap().capture_id, receipt.capture_id);
    assert_eq!(s.capture_by_id(&receipt.capture_id).unwrap().text, req.text);
    req.text.push('！');
    assert_eq!(s.mcp_capture(&req).unwrap_err(), DataError::RequestConflict);
    let key = RecordKey {
        kind: "memory".into(),
        id: receipt.memory_id,
    };
    let jobs = s.organization_jobs(&key).unwrap();
    assert_eq!(jobs.len(), 1);
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
    s.save_conversation_draft(&c, "私有草稿秘密").unwrap();
    s.save_workspace_draft(&WorkspaceDraft {
        conclusion: None,
        key: "capture".into(),
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
            title: "新版".into(),
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
