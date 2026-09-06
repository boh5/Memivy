use memivy_core::{CaptureInput, DataPaths, Error, Store};
use std::{fs, os::unix::fs::PermissionsExt};
use uuid::Uuid;
fn input(text: &str) -> CaptureInput {
    CaptureInput {
        request_id: Uuid::new_v4().to_string(),
        text: text.into(),
        source_app: "阶段一测试".into(),
        project: None,
        session_uri: None,
    }
}
fn setup() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(DataPaths::new(dir.path().into()).unwrap()).unwrap();
    (dir, store)
}

#[test]
fn raw_is_exact_immutable_and_retries_do_not_duplicate() {
    let (_dir, store) = setup();
    let mut request = input("  原话\n不做改写。🙂\n");
    let first = store.capture(request.clone()).unwrap();
    let second = store.capture(request.clone()).unwrap();
    assert_eq!(first.id, second.id);
    assert_eq!(first.text, request.text);
    assert_eq!(first.ai_state, "pending");
    request.text = "修改后的内容".into();
    assert!(matches!(
        store.capture(request),
        Err(Error::RequestConflict)
    ));
    let db = rusqlite::Connection::open(store.paths.database()).unwrap();
    assert!(
        db.execute("UPDATE captures SET text='rewritten'", [])
            .is_err()
    );
    assert_eq!(store.diagnostics().unwrap().count, 1);
    assert_eq!(store.search("", 50).unwrap().items[0].text, first.text);
}

#[test]
fn search_handles_chinese_short_mixed_and_literal_syntax() {
    let (_dir, store) = setup();
    store
        .capture(input(
            "先给结果，再解释配置。https://example.com/a_b?q=100% \"exact\" C++",
        ))
        .unwrap();
    store.capture(input("先给结果，但没有另外那个词")).unwrap();
    store.capture(input("只谈配置，other words")).unwrap();
    for query in [
        "先给结果 配置",
        "配",
        "配置",
        "https://example.com/a_b?q=100%",
        "100%",
        "_",
        "%",
        "\"exact\"",
        "C++",
    ] {
        let page = store.search(query, 50).unwrap();
        assert!(!page.items.is_empty(), "query {query}");
        if query == "先给结果 配置" {
            assert_eq!(page.items.len(), 1);
            assert_eq!(page.strategy, "trigram+literal");
        }
    }
    assert!(store.search("missing 配置", 50).unwrap().items.is_empty());
    // Bound returned records, not the set of records eligible for a short query.
    for _ in 0..75 {
        store.capture(input("new filler record")).unwrap();
    }
    assert_eq!(store.search("配", 1).unwrap().items.len(), 1);
    assert_eq!(store.search("", 500).unwrap().items.len(), 50);
    assert!(store.search(&"x".repeat(513), 50).is_err());
}

#[test]
fn migration_reopens_and_rejects_future_schema() {
    let (_dir, store) = setup();
    store.capture(input("保留")).unwrap();
    let again = Store::open(store.paths.clone()).unwrap();
    let diag = again.diagnostics().unwrap();
    assert_eq!(diag.count, 1);
    assert_eq!(diag.journal_mode, "wal");
    assert_eq!(diag.synchronous, 2);
    assert!(diag.fts5);
    let version: Vec<u32> = diag
        .sqlite_version
        .split('.')
        .map(|v| v.parse().unwrap())
        .collect();
    assert!(
        version.as_slice() >= [3, 51, 3].as_slice(),
        "must bundle patched SQLite: {}",
        diag.sqlite_version
    );
    assert_ne!(diag.sqlite_version, "3.52.0");
    let db = rusqlite::Connection::open(store.paths.database()).unwrap();
    db.pragma_update(None, "user_version", 999).unwrap();
    assert!(matches!(
        Store::open(store.paths.clone()),
        Err(Error::NewerSchema)
    ));
    assert!(matches!(
        store.capture(input("不能写")),
        Err(Error::NewerSchema)
    ));
}

#[test]
fn mcp_switch_fails_closed_and_credentials_stay_separate() {
    let (_dir, store) = setup();
    assert!(matches!(
        store.mcp_capture(input("关闭")),
        Err(Error::McpDisabled)
    ));
    store.paths.set_mcp_enabled(true).unwrap();
    store.mcp_capture(input("开启")).unwrap();
    store.paths.set_mcp_enabled(false).unwrap();
    assert!(matches!(
        store.mcp_capture(input("再关闭")),
        Err(Error::McpDisabled)
    ));
    fs::write(store.paths.root.join("mcp.json"), "broken").unwrap();
    assert!(!store.paths.mcp_enabled());
    fs::write(
        store.paths.model_config(),
        r#"{"api_key":"fixture-secret-never-in-content"}"#,
    )
    .unwrap();
    let marker = b"fixture-secret-never-in-content";
    assert!(
        !fs::read(store.paths.database())
            .unwrap()
            .windows(marker.len())
            .any(|w| w == marker)
    );
    assert_eq!(
        fs::metadata(store.paths.database())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(&store.paths.root)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

#[test]
fn simultaneous_writers_share_one_store_without_overwrite() {
    let (_dir, store) = setup();
    let mut threads = Vec::new();
    for n in 0..8 {
        let store = store.clone();
        threads.push(std::thread::spawn(move || {
            for i in 0..20 {
                store
                    .capture(input(&format!("writer {n} row {i}")))
                    .unwrap();
            }
        }));
    }
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(store.diagnostics().unwrap().count, 160);
}
