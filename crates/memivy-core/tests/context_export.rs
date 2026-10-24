use memivy_core::memory::*;
use memivy_core::model::ProbeError;
use rusqlite::Connection;
use serde_json::{Value, json};

#[allow(dead_code)]
#[path = "support/agent_fixture.rs"]
mod agent_fixture;
use agent_fixture::{Response, fixture};

fn response(answer: Value, finish: &str) -> Response {
    Response {
        status: 200,
        parts: vec![json!({
            "id":"context-fixture", "model":"synthetic-agent",
            "choices":[{"message":{"role":"assistant","content":answer.to_string()},"finish_reason":finish}],
        }).to_string()],
    }
}

fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn setup() -> (tempfile::TempDir, MemoryStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    (dir, store)
}

fn capture(store: &MemoryStore, text: &str, origin: Origin) -> ContextSelection {
    let result = store
        .capture(&CaptureRequest {
            request_id: id(),
            text: text.into(),
            origin,
        })
        .unwrap();
    ContextSelection {
        memory_id: result.memory_id,
        version_id: result.version_id,
    }
}

fn user_origin() -> Origin {
    Origin::User {
        app: "Memivy".into(),
        project: None,
        uri: None,
    }
}

fn selected(store: &MemoryStore) -> ContextSnapshot {
    let selection = capture(
        store,
        "考虑十月上线，尚未决定。小林说：我已经完成演示。\n是否要做网页版？",
        user_origin(),
    );
    store.prepare_context_export(&[selection]).unwrap()
}

fn durable_state(store: &MemoryStore) -> Value {
    let db = Connection::open(store.database_path()).unwrap();
    let tables = [
        "captures",
        "memory_versions",
        "receipts",
        "conversations",
        "messages",
        "workspace_drafts",
    ];
    let counts: Vec<i64> = tables
        .iter()
        .map(|table| {
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap()
        })
        .collect();
    let revision: i64 = db
        .query_row(
            "SELECT revision FROM library_revision WHERE id=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    json!({"counts":counts,"revision":revision})
}

#[test]
fn snapshot_preserves_full_saved_content_order_and_safe_provenance() {
    let (_dir, store) = setup();
    let text = "## Decision\n\nNot approved. [Link](https://example.com)\n\n- [ ] Maybe later.\n";
    let first = capture(
        &store,
        text,
        Origin::Agent {
            app: "Editor".into(),
            project: Some("/Users/example/private-project".into()),
            uri: Some("file:///Users/example/private-project/note.md".into()),
        },
    );
    let second = capture(
        &store,
        "A quotation: ‘I finished the draft,’ Alex said.",
        Origin::User {
            app: "Browser".into(),
            project: Some("Reading".into()),
            uri: Some("https://example.com/article".into()),
        },
    );
    let draft = WorkspaceDraft {
        destination: None,
        key: format!("memory:{}", first.memory_id),
        request_id: id(),
        title: "Unsaved title".into(),
        body: "Unsaved private draft must not be sent".into(),
        expected_version: Some(first.version_id.clone()),
        origin: None,
        context: vec![],
        context_export: None,
    };
    store.save_workspace_draft(&draft).unwrap();
    let before = durable_state(&store);
    let snapshot = store
        .prepare_context_export(&[second.clone(), first.clone()])
        .unwrap();
    assert_eq!(snapshot.items[0].memory_id, second.memory_id);
    assert_eq!(snapshot.items[1].body, text);
    assert_eq!(snapshot.items[1].version_id, first.version_id);
    assert!(matches!(
        &snapshot.items[1].sources[0].origin,
        Origin::Agent {
            project: None,
            uri: None,
            ..
        }
    ));
    assert!(
        matches!(&snapshot.items[0].sources[0].origin, Origin::User { uri: Some(uri), .. } if uri == "https://example.com/article")
    );
    assert!(
        !serde_json::to_string(&snapshot)
            .unwrap()
            .contains("/Users/example")
    );
    store.check_context_export(&snapshot).unwrap();
    assert_eq!(durable_state(&store), before);
    assert_eq!(
        store.workspace_draft(&draft.key).unwrap().unwrap().body,
        draft.body
    );
}

#[test]
fn snapshot_sanitizes_provenance_urls_without_rewriting_saved_material() {
    let (_dir, store) = setup();
    let text = "Keep this quotation: https://example.com/?original=kept#quoted";
    let cases = [
        (
            "https://example.com/article?token=secret#private",
            Some("https://example.com/article"),
        ),
        (
            "http://news.example.com:8080/read?q=private#section",
            Some("http://news.example.com:8080/read"),
        ),
        (
            "https://EXAMPLE.COM./article?secret=1",
            Some("https://example.com./article"),
        ),
        (
            "https://8.8.8.8/read?token=secret",
            Some("https://8.8.8.8/read"),
        ),
        (
            "https://[2606:4700:4700::1111]/read#private",
            Some("https://[2606:4700:4700::1111]/read"),
        ),
        ("https://user:secret@example.com/read", None),
        ("https://user@example.com/read", None),
        ("file:///Users/example/private.md", None),
        ("memivy://memory/private", None),
        ("ftp://example.com/private", None),
        ("//example.com/private", None),
        ("not a URL", None),
        ("http://localhost/private", None),
        ("https://LOCALHOST./private", None),
        ("http://app.localhost/private", None),
        ("http://printer/private", None),
        ("http://printer.local/private", None),
        ("http://notes.internal/private", None),
        ("http://router.home.arpa/private", None),
        ("http://notes.lan/private", None),
        ("http://notes.private/private", None),
        ("http://example.test/private", None),
        ("http://example.invalid/private", None),
        ("http://notes.example/private", None),
        ("http://notes.alt/private", None),
        ("http://0.1.2.3/private", None),
        ("http://10.1.2.3/private", None),
        ("http://100.64.0.1/private", None),
        ("http://100.127.255.254/private", None),
        ("http://127.0.0.1/private", None),
        ("http://127.1/private", None),
        ("http://2130706433/private", None),
        ("http://0x7f000001/private", None),
        ("http://0177.0.0.1/private", None),
        ("http://169.254.169.254/private", None),
        ("http://172.16.0.1/private", None),
        ("http://172.31.255.254/private", None),
        ("http://192.0.0.1/private", None),
        ("http://192.0.2.1/private", None),
        ("http://192.88.99.1/private", None),
        ("http://192.168.1.2/private", None),
        ("http://198.18.0.1/private", None),
        ("http://198.19.255.254/private", None),
        ("http://198.51.100.1/private", None),
        ("http://203.0.113.1/private", None),
        ("http://224.0.0.1/private", None),
        ("http://255.255.255.255/private", None),
        ("http://[::]/private", None),
        ("http://[::1]/private", None),
        ("http://[::ffff:127.0.0.1]/private", None),
        ("http://[fc00::1]/private", None),
        ("http://[fd12::1]/private", None),
        ("http://[fe80::1]/private", None),
        ("http://[fec0::1]/private", None),
        ("http://[ff02::1]/private", None),
        ("http://[64:ff9b::7f00:1]/private", None),
        ("http://[2001::1]/private", None),
        ("http://[2001:db8::1]/private", None),
        ("http://[2002:7f00:1::1]/private", None),
        ("http://[3fff::1]/private", None),
    ];
    for (uri, expected) in cases {
        let origin = Origin::User {
            app: "Browser".into(),
            project: None,
            uri: Some(uri.into()),
        };
        let selection = capture(&store, text, origin.clone());
        let before = durable_state(&store);
        let snapshot = store
            .prepare_context_export(std::slice::from_ref(&selection))
            .unwrap();
        assert!(
            matches!(&snapshot.items[0].sources[0].origin, Origin::User { uri, .. } if uri.as_deref() == expected),
            "unexpected exported URI for {uri}"
        );
        assert_eq!(snapshot.items[0].body, text);
        store.check_context_export(&snapshot).unwrap();
        let mut unsanitized = snapshot.clone();
        if let Origin::User { uri: saved_uri, .. } = &mut unsanitized.items[0].sources[0].origin {
            *saved_uri = Some(uri.into());
        }
        assert_eq!(
            store.check_context_export(&unsanitized).unwrap_err(),
            DataError::Conflict,
            "older previews with private provenance must require refresh"
        );
        let saved = store
            .library_detail(&RecordKey {
                kind: "memory".into(),
                id: selection.memory_id,
            })
            .unwrap();
        let raw = saved.sources[0].capture.as_ref().unwrap();
        assert_eq!(raw.origin, origin, "saved provenance must remain unchanged");
        assert_eq!(raw.text, text);
        assert_eq!(durable_state(&store), before);
    }
}

#[tokio::test]
async fn outgoing_context_request_contains_only_sanitized_provenance_urls() {
    let (_dir, store) = setup();
    let selections: Vec<_> = [
        "https://example.com/article?access_token=secret-query#secret-fragment",
        "http://localhost/private-local-path?token=secret-local",
        "http://192.168.1.2/private-lan-path",
        "http://[fd12::1]/private-ipv6-path",
        "https://secret-user:secret-password@example.com/private-auth-path",
    ]
    .into_iter()
    .map(|uri| {
        capture(
            &store,
            "Selected saved text",
            Origin::Agent {
                app: "Browser".into(),
                project: None,
                uri: Some(uri.into()),
            },
        )
    })
    .collect();
    let snapshot = store.prepare_context_export(&selections).unwrap();
    let before = durable_state(&store);
    let (config, requests, server) = fixture(1, |_, _| {
        response(json!({"body":"Selected saved context. [S1]"}), "stop")
    });
    propose_context_export(&config, &snapshot, "en")
        .await
        .unwrap();
    server.join().unwrap();
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    let encoded = serde_json::to_string(&requests[0]).unwrap();
    assert!(encoded.contains("https://example.com/article"));
    for excluded in [
        "access_token",
        "secret-query",
        "secret-fragment",
        "localhost",
        "private-local-path",
        "secret-local",
        "192.168.1.2",
        "private-lan-path",
        "fd12::1",
        "private-ipv6-path",
        "secret-user",
        "secret-password",
        "private-auth-path",
    ] {
        assert!(
            !encoded.contains(excluded),
            "private provenance leaked: {excluded}"
        );
    }
    assert_eq!(durable_state(&store), before);
    store.check_context_export(&snapshot).unwrap();
}

#[test]
fn rejects_invalid_missing_duplicate_and_oversized_selections_without_truncation() {
    let (_dir, store) = setup();
    assert_eq!(
        store.prepare_context_export(&[]).unwrap_err(),
        DataError::Invalid
    );
    let selection = capture(&store, "Small note", user_origin());
    assert_eq!(
        store
            .prepare_context_export(&[selection.clone(), selection.clone()])
            .unwrap_err(),
        DataError::Invalid
    );
    assert_eq!(
        store
            .prepare_context_export(&[ContextSelection {
                memory_id: "not-an-id".into(),
                version_id: id()
            }])
            .unwrap_err(),
        DataError::Invalid
    );
    assert_eq!(
        store
            .prepare_context_export(&[ContextSelection {
                memory_id: id(),
                version_id: id()
            }])
            .unwrap_err(),
        DataError::Unavailable
    );
    assert_eq!(
        store
            .prepare_context_export(&[ContextSelection {
                memory_id: selection.memory_id,
                version_id: id()
            }])
            .unwrap_err(),
        DataError::Conflict
    );
    let mut twenty = Vec::new();
    for _ in 0..20 {
        twenty.push(capture(&store, "Bounded note", user_origin()));
    }
    assert_eq!(
        store.prepare_context_export(&twenty).unwrap().items.len(),
        20
    );
    twenty.push(capture(&store, "One more", user_origin()));
    assert_eq!(
        store.prepare_context_export(&twenty).unwrap_err(),
        DataError::ContextBudget
    );
    let large = capture(
        &store,
        &"中".repeat(CONTEXT_EXPORT_MAX_BYTES / 3),
        user_origin(),
    );
    assert_eq!(
        store.prepare_context_export(&[large]).unwrap_err(),
        DataError::ContextBudget
    );
}

#[test]
fn rejects_changed_versions_deleted_sources_and_tampered_metadata() {
    let (_dir, store) = setup();
    let original = selected(&store);
    type Mutation = Box<dyn Fn(&mut ContextMemory)>;
    let mutations: Vec<Mutation> = vec![
        Box::new(|item| item.body.push_str("Forged")),
        Box::new(|item| item.title.push_str("Forged")),
        Box::new(|item| item.created_at += 1),
        Box::new(|item| item.sources[0].recorded_at += 1),
        Box::new(|item| {
            item.sources[0].origin = Origin::Agent {
                app: "Other".into(),
                project: None,
                uri: None,
            }
        }),
        Box::new(|item| item.sources.clear()),
    ];
    for mutate in mutations {
        let mut changed = original.clone();
        mutate(&mut changed.items[0]);
        assert_eq!(
            store.check_context_export(&changed).unwrap_err(),
            DataError::Conflict
        );
    }
    let item = &original.items[0];
    let edited = store
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: item.memory_id.clone(),
            expected_version: item.version_id.clone(),
            title: "Updated".into(),
            body: "Later decision".into(),
        })
        .unwrap();
    assert_eq!(
        store.check_context_export(&original).unwrap_err(),
        DataError::Conflict
    );
    let current = store
        .prepare_context_export(&[ContextSelection {
            memory_id: item.memory_id.clone(),
            version_id: edited.after_version.unwrap(),
        }])
        .unwrap();
    let db = Connection::open(store.database_path()).unwrap();
    db.execute(
        "UPDATE capture_state SET availability='purged' WHERE capture_id=?",
        [&item.sources[0].capture_id],
    )
    .unwrap();
    assert_eq!(
        store.check_context_export(&current).unwrap_err(),
        DataError::Unavailable
    );
}

#[test]
fn deleted_selected_memories_are_unavailable() {
    let (_dir, store) = setup();
    let snapshot = selected(&store);
    let item = &snapshot.items[0];
    store
        .trash_memory(&item.memory_id, &item.version_id)
        .unwrap();
    assert_eq!(
        store.check_context_export(&snapshot).unwrap_err(),
        DataError::Unavailable
    );
}

#[tokio::test]
async fn sends_one_complete_read_only_request_with_pinned_sources_and_language() {
    let (_dir, store) = setup();
    let snapshot = selected(&store);
    let before = durable_state(&store);
    let expected =
        "## 背景\n\n- 十月上线仍是考虑中的方案，尚未决定。[S1]\n- 小林表示自己完成了演示。[S1]";
    let answer = expected.to_string();
    let (config, requests, server) =
        fixture(1, move |_, _| response(json!({"body":answer}), "stop"));
    let result = propose_context_export(&config, &snapshot, "zh")
        .await
        .unwrap();
    server.join().unwrap();
    assert_eq!(result, expected);
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert!(
        request
            .get("tools")
            .is_none_or(|tools| tools.as_array().is_some_and(Vec::is_empty))
    );
    let encoded = serde_json::to_string(request).unwrap();
    assert!(encoded.contains("Simplified Chinese"));
    assert!(encoded.contains("preserve negation"));
    assert!(encoded.contains("an unanswered question does not establish an unresolved issue"));
    assert!(encoded.contains(&snapshot.items[0].version_id));
    assert!(encoded.contains("尚未决定"));
    assert!(encoded.contains("是否要做网页版"));
    assert_eq!(durable_state(&store), before);
    store.check_context_export(&snapshot).unwrap();
}

#[tokio::test]
async fn rejects_missing_invented_or_uncited_output_and_provider_truncation() {
    let (_dir, store) = setup();
    let snapshot = selected(&store);
    let cases = vec![
        (
            json!({"body":"No citations."}),
            "stop",
            ProbeError::InvalidResponse,
        ),
        (
            json!({"body":"Invented source. [S2]"}),
            "stop",
            ProbeError::InvalidResponse,
        ),
        (
            json!({"body":"Invalid source. [S01]"}),
            "stop",
            ProbeError::InvalidResponse,
        ),
        (
            json!({"body":"Invalid source. [Sx]"}),
            "stop",
            ProbeError::InvalidResponse,
        ),
        (
            json!({"body":"Invalid source. [S0]"}),
            "stop",
            ProbeError::InvalidResponse,
        ),
        (
            json!({"body":"Uncited paragraph.\n\nCited paragraph. [S1]"}),
            "stop",
            ProbeError::InvalidResponse,
        ),
        (
            json!({"body":"- Uncited item.\n- Cited item. [S1]"}),
            "stop",
            ProbeError::InvalidResponse,
        ),
        (
            json!({"body":"Cited paragraph. [S1]\n\n- Uncited item."}),
            "stop",
            ProbeError::InvalidResponse,
        ),
        (json!({"body":"[S1]"}), "stop", ProbeError::InvalidResponse),
        (
            json!({"body":"# Only a heading [S1]"}),
            "stop",
            ProbeError::InvalidResponse,
        ),
        (
            json!({"body":"Valid text. [S1]", "extra":"ignored"}),
            "stop",
            ProbeError::InvalidResponse,
        ),
        (
            json!({"body":format!("{} [S1]", "a".repeat(CONTEXT_EXPORT_MAX_BYTES))}),
            "stop",
            ProbeError::InvalidResponse,
        ),
        (
            json!({"body":"Partial text. [S1]"}),
            "length",
            ProbeError::Truncated,
        ),
    ];
    let before = durable_state(&store);
    for (answer, finish, expected) in cases {
        let (config, _, server) = fixture(1, move |_, _| response(answer.clone(), finish));
        assert_eq!(
            propose_context_export(&config, &snapshot, "en")
                .await
                .unwrap_err(),
            expected
        );
        server.join().unwrap();
    }
    assert_eq!(durable_state(&store), before);
}

#[tokio::test]
async fn invalid_budget_or_language_never_reaches_model() {
    let (_dir, store) = setup();
    let snapshot = selected(&store);
    let (config, requests, _server) = fixture(0, |_, _| unreachable!());
    assert_eq!(
        propose_context_export(&config, &snapshot, "invented")
            .await
            .unwrap_err(),
        ProbeError::InvalidResponse
    );
    let mut oversized = snapshot;
    oversized.items[0].body = "x".repeat(CONTEXT_EXPORT_MAX_BYTES);
    assert_eq!(
        propose_context_export(&config, &oversized, "en")
            .await
            .unwrap_err(),
        ProbeError::InvalidResponse
    );
    assert!(requests.lock().unwrap().is_empty());
}
