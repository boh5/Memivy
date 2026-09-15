use memivy_core::memory::*;
use uuid::Uuid;
fn id() -> String {
    Uuid::new_v4().to_string()
}

#[test]
fn discussion_draft_keeps_materials_and_original_history_pages_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let captured = store
        .capture(&CaptureRequest {
            request_id: id(),
            text: "资料上下文".into(),
            origin: Origin::User {
                app: "QA".into(),
                project: None,
                uri: None,
            },
        })
        .unwrap();
    let topic = id();
    store
        .create_conversation(&topic, "Continue discussion")
        .unwrap();
    let draft = WorkspaceDraft {
        destination: None,
        key: format!("discussion:{topic}"),
        request_id: id(),
        title: String::new(),
        body: "还没问的问题".into(),
        expected_version: None,
        origin: None,
        context: vec![SourceRef::Version(captured.version_id)],
    };
    store.save_workspace_draft(&draft).unwrap();
    for n in 0..25 {
        let input = id();
        let attempt = id();
        store
            .begin_agent_input(&input, &attempt, &topic, &format!("问题{n}"), &[], None)
            .unwrap();
        store
            .append_agent_text(&input, &attempt, &format!("建议{n}"))
            .unwrap();
        store.finish_agent_input(&input, &attempt, &[]).unwrap();
    }
    drop(store);
    let store = MemoryStore::open(dir.path()).unwrap();
    let loaded = store.workspace_draft(&draft.key).unwrap().unwrap();
    assert_eq!(loaded.body, draft.body);
    assert_eq!(loaded.context, draft.context);
    assert!(
        store
            .search(&SearchRequest::text(&draft.body, 20))
            .unwrap()
            .items
            .is_empty()
    );
    let latest = store.recent_messages(&topic, None, 40).unwrap();
    assert_eq!(latest.len(), 40);
    assert_eq!(latest.last().unwrap().text, "建议24");
    let older = store
        .recent_messages(&topic, Some(latest[0].seq), 40)
        .unwrap();
    assert_eq!(older.len(), 10);
    assert!(older.last().unwrap().seq < latest[0].seq);
    let all = store.messages(&topic, 0, 100).unwrap();
    assert_eq!(all.len(), 50);
}

#[path = "support/agent_fixture.rs"]
pub mod agent_fixture;
use agent_fixture::{Response, fixture, sse_text};

#[tokio::test]
async fn actual_budget_compaction_keeps_early_conditions_and_original_messages_readable() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let topic = id();
    store.create_conversation(&topic, "长期讨论").unwrap();
    for n in 0..6 {
        let input = id();
        let attempt = id();
        let text = if n == 0 {
            "最初条件：每周8小时，预算4800元，不上传录音，是否收费尚未决定。".into()
        } else {
            format!("背景{n}：{}", "仅比较方案，不宣布新事实或决定。".repeat(70))
        };
        store
            .begin_agent_input(&input, &attempt, &topic, &text, &[], None)
            .unwrap();
        store
            .append_agent_text(&input, &attempt, "继续比较备选方案，保留原先条件。")
            .unwrap();
        store.finish_agent_input(&input, &attempt, &[]).unwrap();
    }
    let (config, requests, server) = fixture(3, |index, request| match index {
        0 => {
            assert!(
                request["messages"][0]["content"][0]["text"]
                    .as_str()
                    .unwrap()
                    .contains("Compress earlier")
            );
            assert!(request.to_string().contains("4800"));
            Response::stream(sse_text(
                "用户条件：每周8小时，预算4800元；不上传录音；是否收费尚未决定。后续仅比较备选，没有作新决定。",
            ))
        }
        1 => {
            let wire = request.to_string();
            assert!(wire.contains("earlier_summary"));
            assert!(wire.contains("4800"));
            Response::stream(sse_text(
                "最初条件是每周8小时、预算4800元，不上传录音；是否收费尚未决定。",
            ))
        }
        _ => Response::stream(sse_text("[\"如何在预算内验证？\",\"怎样比较收费方案？\"]")),
    });

    let input = id();
    let attempt = id();
    store
        .begin_agent_input(&input, &attempt, &topic, "最初的条件是什么？", &[], None)
        .unwrap();
    store
        .run_discussion(&config, &input, &attempt, "zh-CN", |_| {})
        .await
        .unwrap();
    server.join().unwrap();
    let context = store.agent_conversation_context(&topic).unwrap();
    assert!(context.summary_through_seq > 0);
    assert!(context.summary.contains("4800"));
    assert_eq!(store.messages(&topic, 0, 100).unwrap().len(), 14);
    assert!(
        store
            .library(&LibraryQuery::default())
            .unwrap()
            .items
            .is_empty()
    );
    assert_eq!(requests.lock().unwrap().len(), 3);
}
