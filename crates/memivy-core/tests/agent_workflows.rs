use memivy_core::memory::*;
use serde_json::{Value, json};

fn checkpoint_calls(value: &Value) -> Vec<memivy_core::model::ToolCall> {
    match serde_json::from_value::<memivy_core::model::Message>(value.clone()).unwrap() {
        memivy_core::model::Message::Assistant { content, .. } => content
            .into_iter()
            .filter_map(|p| match p {
                memivy_core::model::AssistantContent::ToolCall(call) => Some(call),
                _ => None,
            })
            .collect(),
        _ => vec![],
    }
}
fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
#[path = "support/agent_fixture.rs"]
mod agent_fixture;
use agent_fixture::{Response, fixture, sse_delta as delta, sse_text as text, sse_tool as call};
fn setup() -> (tempfile::TempDir, MemoryStore, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let conversation = id();
    store
        .create_conversation(&conversation, "Agent QA")
        .unwrap();
    (dir, store, conversation)
}

fn begin(store: &MemoryStore, conversation: &str, text: &str, focus: &[String]) -> AgentExecution {
    store
        .begin_agent_input(&id(), &id(), conversation, text, focus, None)
        .unwrap()
}
fn suggestions() -> String {
    text("[\"还需要验证哪个假设？\",\"这个方向与原来的计划有什么关系？\"]")
}
fn last_content(request: &Value) -> Value {
    serde_json::from_str(
        request["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap()
}
fn count(store: &MemoryStore, table: &str) -> i64 {
    rusqlite::Connection::open(store.database_path())
        .unwrap()
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[tokio::test]
async fn actual_requests_write_and_generate_followups_without_control_tools_or_capability_cache() {
    for input in [
        "我每周可投入8小时。",
        "我每周可投入8小时。这对计划有什么影响？",
    ] {
        let (dir, store, conversation) = setup();
        // Old cache files and retired classification fields must not gate actual requests.
        std::fs::write(
            dir.path().join("model-capabilities.json"),
            "invalid old cache",
        )
        .unwrap();
        let run = begin(&store, &conversation, input, &[]);
        let db = rusqlite::Connection::open(store.database_path()).unwrap();
        db.execute(
            "UPDATE conversations SET memory_paused=1 WHERE id=?",
            [&conversation],
        )
        .unwrap();
        db.execute(
            "UPDATE turns SET record_only=1,maintenance_paused=1 WHERE id=?",
            [&run.input_id],
        )
        .unwrap();
        let source = run.user_message_id.clone();
        let (config, requests, server) = fixture(3, move |index, request| {
            Response::stream(match index {
                0 => {
                    let context = last_content(request);
                    assert_eq!(context["source_message_id"], source);
                    assert!(context.get("memory_maintenance_paused").is_none());
                    assert!(
                        !request["tools"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|t| t["function"]["name"] == "set_turn_options")
                    );
                    call(
                        "write-hours",
                        "write_memory",
                        json!({"destination":{"kind":"new"},"title":"每周可投入时间","parts":[{"text":"每周可投入8小时。","sources":[{"source_id":source,"quote":"我每周可投入8小时。"}]}]}),
                    )
                }
                1 => {
                    assert_eq!(last_content(request)["receipt"]["status"], "applied");
                    text("已记下：每周可投入8小时。")
                }
                _ => suggestions(),
            })
        });
        store
            .run_discussion(&config, &run.input_id, &run.attempt_id, "zh-CN", |_| {})
            .await
            .unwrap();
        server.join().unwrap();
        let result = store.agent_execution(&run.input_id).unwrap();
        assert_eq!(result.state, "complete");
        assert_eq!(result.follow_ups.len(), 2);
        assert_eq!(count(&store, "memories"), 1);
        assert_eq!(count(&store, "captures"), 1);
        let receipt = store.agent_input_receipts(&run.input_id).unwrap();
        assert_eq!(receipt.len(), 1);
        assert_eq!(receipt[0].status, "applied");
        assert_eq!(requests.lock().unwrap().len(), 3);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("model-capabilities.json")).unwrap(),
            "invalid old cache"
        );
    }
}

#[tokio::test]
async fn no_memory_request_is_visible_to_agent_without_a_control_step() {
    let (_dir, store, conversation) = setup();
    let run = begin(&store, &conversation, "这轮不要记忆，只帮我比较想法。", &[]);
    let (config, requests, server) = fixture(2, |index, request| {
        Response::stream(if index == 0 {
            assert_eq!(
                last_content(request)["current_message"],
                "这轮不要记忆，只帮我比较想法。"
            );
            assert!(request.to_string().contains("do not call write_memory"));
            text("可以先比较成本和投入时间。")
        } else {
            suggestions()
        })
    });
    store
        .run_discussion(&config, &run.input_id, &run.attempt_id, "zh-CN", |_| {})
        .await
        .unwrap();
    server.join().unwrap();
    assert_eq!(count(&store, "memories"), 0);
    assert_eq!(
        store
            .agent_execution(&run.input_id)
            .unwrap()
            .follow_ups
            .len(),
        2
    );
    assert_eq!(requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn committed_tool_without_checkpoint_result_resumes_only_after_that_tool() {
    let (_dir, store, conversation) = setup();
    let run = begin(
        &store,
        &conversation,
        "决定先验证反馈收集，还没有上线。然后怎么做？",
        &[],
    );
    let args = json!({"destination":{"kind":"new"},"title":"反馈验证","parts":[{"text":"决定验证反馈收集，尚未上线。","sources":[{"source_id":run.user_message_id,"quote":run.input_text}]}]});
    let reasoning = json!({
        "reasoning":"Legacy provider reasoning",
        "reasoning_content":"Legacy canonical reasoning",
        "reasoning_details":[
            {"type":"reasoning.text","id":"thinking-7","format":"anthropic-claude-v1","index":3,"text":"Signed thought","signature":"original-signature"},
            {"type":"reasoning.encrypted","id":"rs_8","format":"openai-responses-v1","index":4,"data":"original-encrypted-data"}
        ]
    });
    let mut protocol = vec![
        json!({"role":"user","content":run.input_text}),
        json!({"role":"assistant","content":"先记下这个决定。","tool_calls":[{"id":"committed-call","type":"function","function":{"name":"write_memory","arguments":args.to_string()}}]}),
    ];
    protocol[1]
        .as_object_mut()
        .unwrap()
        .extend(reasoning.as_object().unwrap().clone());
    store
        .append_agent_text(&run.input_id, &run.attempt_id, "先记下这个决定。")
        .unwrap();
    store
        .checkpoint_agent(&run.input_id, &run.attempt_id, &protocol)
        .unwrap();
    let op = store
        .stage_agent_operation(
            &run.input_id,
            &run.attempt_id,
            "committed-call",
            "write_memory",
            &args,
        )
        .unwrap();
    let committed = store
        .apply_agent_memory(
            &run.input_id,
            &run.attempt_id,
            &op.operation_id,
            &serde_json::from_value(args).unwrap(),
        )
        .unwrap();
    // SQLite COMMIT happened, but neither model tool result nor UI callback did.
    store
        .stop_agent_input(&run.input_id, &run.attempt_id, "failed", Some("network"))
        .unwrap();
    let retry = store.retry_agent_input(&run.input_id, &id()).unwrap();
    let operation_id = op.operation_id.clone();
    let receipt_id = committed.receipt.unwrap().request_id;
    let (config, requests, server) = fixture(2, move |index, request| {
        Response::stream(if index == 0 {
            let messages = request["messages"].as_array().unwrap();
            assert_eq!(messages.len(), 3);
            assert_eq!(messages[1]["tool_calls"][0]["id"], "committed-call");
            assert_eq!(messages[2]["tool_call_id"], "committed-call");
            for (key, value) in reasoning.as_object().unwrap() {
                assert_eq!(&messages[1][key], value, "legacy reasoning field {key}");
            }
            assert!(!request.to_string().contains("_memivy_openai_reasoning"));
            let result = last_content(request);
            assert_eq!(result["receipt"]["request_id"], receipt_id);
            text("下一步先验证需求。")
        } else {
            suggestions()
        })
    });

    store
        .run_discussion(&config, &retry.input_id, &retry.attempt_id, "zh-CN", |_| {})
        .await
        .unwrap();
    server.join().unwrap();
    let result = store.agent_execution(&run.input_id).unwrap();
    assert_eq!(result.state, "complete");
    assert_eq!(result.operations.len(), 1);
    assert_eq!(result.operations[0].operation_id, operation_id);
    assert_eq!(result.text.matches("先记下这个决定。").count(), 1);
    assert_eq!(count(&store, "messages"), 2);
    assert_eq!(count(&store, "captures"), 1);
    assert_eq!(count(&store, "memories"), 1);
    assert_eq!(count(&store, "receipts"), 1);
    assert_eq!(requests.lock().unwrap().len(), 2);
    assert_eq!(
        store
            .append_agent_text(&run.input_id, &run.attempt_id, "迟到结果")
            .unwrap_err(),
        DataError::Conflict
    );
}

#[tokio::test]
async fn streaming_cancellation_retains_visible_prefix_and_stops_before_tools() {
    let (_dir, store, conversation) = setup();
    let run = begin(&store, &conversation, "请分析这个方向。", &[]);
    let (config, requests, server) = fixture(1, |_, _| Response {
        status: 200,
        parts: vec![
            delta(
                json!({"role":"assistant","content":"已显示的前半段。"}),
                json!(null),
            ),
            delta(json!({"content":"不应追加的后半段。"}), json!(null)),
            delta(json!({}), json!("stop")) + "data: [DONE]\n\n",
        ],
    });

    let result = store
        .run_discussion(&config, &run.input_id, &run.attempt_id, "zh-CN", |_| {
            let current = store.agent_execution(&run.input_id).unwrap();
            if current.state == "processing" && !current.text.is_empty() {
                store
                    .stop_agent_input(&run.input_id, &run.attempt_id, "cancelled", None)
                    .unwrap();
            }
        })
        .await;
    assert!(result.is_err());
    server.join().unwrap();
    let saved = store.agent_execution(&run.input_id).unwrap();
    assert_eq!(saved.state, "cancelled");
    assert_eq!(saved.text, "已显示的前半段。");
    assert_eq!(count(&store, "memories"), 0);
    assert_eq!(requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn cancellation_after_first_commit_preserves_receipt_and_stops_pending_write_across_reopen() {
    let (dir, store, conversation) = setup();
    let run = begin(
        &store,
        &conversation,
        "记录两个想法：先访谈用户，之后验证定价；都还没决定。",
        &[],
    );
    let source = run.user_message_id.clone();
    let source_quote = run.input_text.clone();
    let displayed_text = "你提出了两个尚未决定的想法，我先逐条记录。";
    let first_body = "考虑先访谈用户，尚未决定。";
    let (config, requests, server) = fixture(1, move |_, _| Response {
        status: 200,
        parts: vec![
            delta(
                json!({"role":"assistant","content":displayed_text}),
                json!(null),
            ),
            delta(
                json!({"tool_calls":[
                    {"index":0,"id":"first-write","type":"function","function":{
                        "name":"write_memory","arguments":json!({"destination":{"kind":"new"},"title":"用户访谈想法","parts":[{"text":first_body,"sources":[{"source_id":source,"quote":source_quote}]}]}).to_string()
                    }},
                    {"index":1,"id":"pending-write","type":"function","function":{
                        "name":"write_memory","arguments":json!({"destination":{"kind":"new"},"title":"定价验证想法","parts":[{"text":"考虑验证定价，尚未决定。","sources":[{"source_id":source,"quote":source_quote}]}]}).to_string()
                    }}
                ]}),
                json!("tool_calls"),
            ) + "data: [DONE]\n\n",
        ],
    });

    let mut streamed_before_commit = false;
    let mut successful_updates = 0;
    let outcome = store
        .run_discussion(
            &config,
            &run.input_id,
            &run.attempt_id,
            "zh-CN",
            |changed| {
                let current = store.agent_execution(&run.input_id).unwrap();
                if current.text == displayed_text && current.operations.is_empty() {
                    streamed_before_commit = true;
                }
                if changed {
                    successful_updates += 1;
                    assert_eq!(current.state, "processing");
                    assert_eq!(current.operations.len(), 1);
                    assert_eq!(current.operations[0].call_id, "first-write");
                    assert_eq!(
                        current.operations[0].receipt.as_ref().unwrap().status,
                        "applied"
                    );
                    let pending = current.protocol.last().unwrap();
                    assert_eq!(checkpoint_calls(pending)[1].id.as_str(), "pending-write");
                    assert!(!current.protocol.iter().any(|m| {
                        m["content"]
                            .as_array()
                            .is_some_and(|parts| parts.iter().any(|p| p["type"] == "toolresult"))
                    }));
                    store
                        .stop_agent_input(&run.input_id, &run.attempt_id, "cancelled", None)
                        .unwrap();
                }
            },
        )
        .await;
    assert!(outcome.is_err());
    server.join().unwrap();
    assert!(streamed_before_commit);
    assert_eq!(successful_updates, 1);
    assert_eq!(requests.lock().unwrap().len(), 1);
    let stopped = store.agent_execution(&run.input_id).unwrap();
    assert_eq!(stopped.state, "cancelled");
    assert_eq!(stopped.text, displayed_text);
    assert!(stopped.follow_ups.is_empty());
    let receipts = store.agent_input_receipts(&run.input_id).unwrap();
    assert_eq!(receipts.len(), 1);
    let receipt_id = receipts[0].request_id.clone();
    let memory_id = receipts[0].memory_id.clone().unwrap();
    let committed_version = store.memory(&memory_id).unwrap().current;
    assert_eq!(committed_version.body, first_body);
    assert_eq!(count(&store, "memories"), 1);
    assert_eq!(count(&store, "captures"), 1);
    assert_eq!(count(&store, "receipts"), 1);
    assert_eq!(count(&store, "organization_jobs"), 0);
    drop(store);

    let reopened = MemoryStore::open(dir.path()).unwrap();
    let restored = reopened.agent_execution(&run.input_id).unwrap();
    // An intentional cancellation remains cancelled; reopening does not claim
    // completion, drop the successful receipt, or automatically run pending work.
    assert_eq!(restored.state, "cancelled");
    assert_eq!(restored.text, displayed_text);
    assert_eq!(restored.user_message_id, run.user_message_id);
    assert_eq!(restored.operations.len(), 1);
    assert_eq!(restored.operations[0].call_id, "first-write");
    assert!(restored.operations[0].result.is_some());
    assert_eq!(
        restored.operations[0].receipt.as_ref().unwrap().request_id,
        receipt_id
    );
    let turn = reopened.turn(&run.input_id).unwrap();
    assert_eq!(turn.assistant.status, "cancelled");
    assert_eq!(turn.assistant.receipts.len(), 1);
    assert_eq!(turn.assistant.receipts[0].status, "applied");
    assert_eq!(turn.user.text, run.input_text);
    assert_eq!(
        reopened.memory(&memory_id).unwrap().current.id,
        committed_version.id
    );
    assert_eq!(
        reopened.memory(&memory_id).unwrap().current.body,
        first_body
    );
    assert_eq!(count(&reopened, "messages"), 2);
    assert_eq!(count(&reopened, "memories"), 1);
    assert_eq!(count(&reopened, "captures"), 1);
    assert_eq!(count(&reopened, "receipts"), 1);
    assert!(
        reopened
            .run_discussion(&config, &run.input_id, &run.attempt_id, "zh-CN", |_| {})
            .await
            .is_err()
    );
    assert_eq!(
        reopened.agent_execution(&run.input_id).unwrap().state,
        "cancelled"
    );
    assert_eq!(count(&reopened, "memories"), 1);
    reopened.check_integrity().unwrap();
}

#[tokio::test]
async fn reading_v1_then_writing_v2_keeps_the_cited_v1_available() {
    let (_dir, store, conversation) = setup();
    let raw = store
        .capture(&CaptureRequest {
            request_id: id(),
            text: "旧计划：先做社区。".into(),
            origin: Origin::User {
                app: "QA".into(),
                project: None,
                uri: None,
            },
        })
        .unwrap();
    let run = begin(
        &store,
        &conversation,
        "决定改成先做反馈收集，社区暂缓。和旧计划有什么变化？",
        std::slice::from_ref(&raw.memory_id),
    );
    let source = run.user_message_id.clone();
    let source_quote = run.input_text.clone();
    let target = raw.memory_id.clone();
    let old_version = raw.version_id.clone();
    let cite_version = old_version.clone();
    let (config, _, server) = fixture(3, move |index, request| {
        Response::stream(match index {
            0 => {
                assert!(request.to_string().contains(&old_version));
                call(
                    "write-1",
                    "write_memory",
                    json!({"destination":{"kind":"existing","memory_id":target,"expected_version":old_version},"title":"产品计划","parts":[{"text":"之前计划先做社区。","sources":[{"source_id":old_version,"quote":"旧计划：先做社区。"}]},{"text":"现在决定先做反馈收集，社区暂缓。","sources":[{"source_id":source,"quote":source_quote}]}]}),
                )
            }
            1 => text(&format!(
                "[旧计划](memivy://source/version/{old_version})是先做社区；现在决定先验证反馈收集。"
            )),
            _ => suggestions(),
        })
    });

    store
        .run_discussion(&config, &run.input_id, &run.attempt_id, "zh-CN", |_| {})
        .await
        .unwrap();
    server.join().unwrap();
    let current = store.memory(&raw.memory_id).unwrap().current;
    assert_ne!(current.id, raw.version_id);
    assert!(current.body.contains("社区暂缓"));
    let citation = store
        .discussion_excerpt(
            &run.assistant_message_id,
            &SourceRef::Version(cite_version.clone()),
        )
        .unwrap();
    assert_eq!(citation.text, "旧计划：先做社区。");
    assert_eq!(
        store
            .search(&SearchRequest::text("反馈收集", 8))
            .unwrap()
            .items[0]
            .version_id,
        current.id
    );
    store.trash_memory(&raw.memory_id, &current.id).unwrap();
    assert!(matches!(
        store.discussion_excerpt(&run.assistant_message_id, &SourceRef::Version(cite_version)),
        Err(DataError::Unavailable)
    ));
}

#[tokio::test]
async fn citation_keeps_longer_reread_and_disjoint_tail_without_exposing_unread_gap() {
    let (_dir, store, conversation) = setup();
    let near = "VERIFIED_NEAR_CODE_Z91";
    let far = "VERIFIED_FAR_CODE_R28";
    let unread = "UNREAD_MIDDLE_MARKER";
    let mut body = "太阳能项目资料。".to_string() + &"甲".repeat(2200) + near;
    body += &"乙".repeat(1200);
    body += unread;
    body += &"丙".repeat(1200);
    let far_start = body.chars().count();
    body += far;
    let captured = store
        .capture(&CaptureRequest {
            request_id: id(),
            text: body,
            origin: Origin::User {
                app: "QA".into(),
                project: None,
                uri: None,
            },
        })
        .unwrap();
    let run = begin(
        &store,
        &conversation,
        "请读取太阳能项目资料，告诉我两个编码。",
        &[],
    );
    let memory = captured.memory_id.clone();
    let version = captured.version_id.clone();
    let (config, requests, server) = fixture(4, move |index, request| {
        Response::stream(match index {
            0 => {
                let context = last_content(request);
                let evidence = &context["memory_context"]["global"][0]["evidence"];
                assert_eq!(evidence["start"], 0);
                assert_eq!(evidence["text"].as_str().unwrap().chars().count(), 900);
                call(
                    "longer-read",
                    "read_memory",
                    json!({"memory_id":memory,"view":"current","source_id":null,"start_char":0,"max_chars":3000}),
                )
            }
            1 => {
                let result = last_content(request);
                assert_eq!(result["evidence"]["start"], 0);
                assert!(result["evidence"]["text"].as_str().unwrap().contains(near));
                assert!(!request.to_string().contains(unread));
                call(
                    "far-read",
                    "read_memory",
                    json!({"memory_id":memory,"view":"current","source_id":null,"start_char":far_start,"max_chars":1000}),
                )
            }
            2 => {
                assert!(
                    last_content(request)["evidence"]["text"]
                        .as_str()
                        .unwrap()
                        .contains(far)
                );
                assert!(!request.to_string().contains(unread));
                text(&format!(
                    "[项目资料](memivy://source/version/{version})中的编码是 {near} 和 {far}。"
                ))
            }
            _ => suggestions(),
        })
    });

    store
        .run_discussion(&config, &run.input_id, &run.attempt_id, "zh-CN", |_| {})
        .await
        .unwrap();
    server.join().unwrap();
    let excerpt = store
        .discussion_excerpt(
            &run.assistant_message_id,
            &SourceRef::Version(captured.version_id),
        )
        .unwrap();
    assert_eq!(excerpt.start, 0);
    assert_eq!(excerpt.text.chars().count(), 3000);
    assert!(excerpt.text.contains(near));
    assert_eq!(excerpt.additional_spans.len(), 1);
    assert_eq!(excerpt.additional_spans[0].start, far_start);
    assert!(excerpt.additional_spans[0].text.contains(far));
    assert!(!excerpt.text.contains(unread));
    assert!(
        excerpt
            .additional_spans
            .iter()
            .all(|span| !span.text.contains(unread))
    );
    assert_eq!(requests.lock().unwrap().len(), 4);
}

#[tokio::test]
async fn actual_protocol_failure_is_explicit_without_a_preflight_request() {
    let (_dir, store, conversation) = setup();
    let run = begin(&store, &conversation, "今天想到一个方向。", &[]);
    let (config, requests, server) = fixture(1, |_, _| Response {
        status: 422,
        parts: vec![],
    });

    assert!(matches!(
        store
            .run_discussion(&config, &run.input_id, &run.attempt_id, "zh-CN", |_| {})
            .await,
        Err(Failure::InvalidAnswer)
    ));
    server.join().unwrap();
    assert_eq!(requests.lock().unwrap().len(), 1);
    assert_eq!(
        store.agent_execution(&run.input_id).unwrap().input_text,
        "今天想到一个方向。"
    );
    assert_eq!(count(&store, "memories"), 0);
}

#[tokio::test]
async fn suggestions_failure_does_not_change_successful_answer_or_receipts() {
    let (_dir, store, conversation) = setup();
    let run = begin(&store, &conversation, "下一步需要验证什么？", &[]);
    let (config, _, server) = fixture(2, |index, _| {
        if index == 0 {
            Response::stream(text("可以先验证需求。"))
        } else {
            Response {
                status: 500,
                parts: vec!["synthetic failure".into()],
            }
        }
    });

    store
        .run_discussion(&config, &run.input_id, &run.attempt_id, "zh-CN", |_| {})
        .await
        .unwrap();
    server.join().unwrap();
    let result = store.agent_execution(&run.input_id).unwrap();
    assert_eq!(result.state, "complete");
    assert_eq!(result.text, "可以先验证需求。");
    assert!(result.follow_ups.is_empty());
    assert_eq!(count(&store, "memories"), 0);
}

async fn pending_write_rechecks_the_persisted_request_view(prune: bool) {
    let (dir, store, conversation) = setup();
    let original = format!("{}尾部条件：不得上传录音。", "背景。".repeat(2100));
    let captured = store
        .capture(&CaptureRequest {
            request_id: id(),
            text: original.clone(),
            origin: Origin::User {
                app: "QA".into(),
                project: None,
                uri: None,
            },
        })
        .unwrap();
    let run = begin(&store, &conversation, "补充：需要支持离线。", &[]);
    let current = store.memory(&captured.memory_id).unwrap().current;
    let mut protocol = vec![json!({"role":"user","content":json!({
        "source_message_id":run.user_message_id,
        "current_message":run.input_text,
        "earlier_context":if prune { "x".repeat(44_000) } else { String::new() },
    }).to_string()})];
    // This is a real checkpoint shape after three paged reads. A later request
    // can release older tool bodies while the durable protocol keeps them.
    for start in [0, 3000, 6000] {
        let call_id = format!("read-{start}");
        protocol.push(json!({"role":"assistant","content":null,"tool_calls":[{
            "id":call_id,"type":"function","function":{"name":"read_memory",
            "arguments":json!({"memory_id":captured.memory_id,"view":"current","source_id":null,"start_char":start,"max_chars":3000}).to_string()}
        }]}));
        let evidence = Evidence {
            source: SourceRef::Version(current.id.clone()),
            title: current.title.clone(),
            text: original.chars().skip(start).take(3000).collect(),
            truncated: true,
            recorded_at: current.created_at,
            current: true,
            start,
            additional_spans: vec![],
        };
        protocol.push(json!({"role":"tool","tool_call_id":call_id,
            "content":json!({"memory_id":captured.memory_id,"evidence":evidence}).to_string()}));
    }
    store
        .checkpoint_agent(&run.input_id, &run.attempt_id, &protocol)
        .unwrap();
    let source = run.user_message_id.clone();
    let source_quote = run.input_text.clone();
    let memory_id = captured.memory_id.clone();
    let version_id = captured.version_id.clone();
    let parts = if prune {
        json!([{"text":"危险的片段覆盖。","sources":[{"source_id":source,"quote":source_quote}]}])
    } else {
        json!([{"text":original,"sources":[]},{"text":"需要支持离线。","sources":[{"source_id":source,"quote":source_quote}]}])
    };
    let (config, requests, server) = fixture(3, move |index, request| {
        Response::stream(match index {
            0 => {
                let first_read: Value =
                    serde_json::from_str(request["messages"][2]["content"].as_str().unwrap())
                        .unwrap();
                assert_eq!(first_read["evidence"]["text"].is_null(), prune);
                call(
                    "pending-write",
                    "write_memory",
                    json!({
                        "destination":{"kind":"existing","memory_id":memory_id,"expected_version":version_id},
                        "title":"离线方案","parts":parts,
                    }),
                )
            }
            1 => {
                // Retry executes the saved call before a new model request.
                let result = last_content(request);
                if prune {
                    assert_eq!(result["applied"], false);
                    assert!(result["error"].as_str().unwrap().contains("not visible"));
                } else {
                    assert_eq!(result["receipt"]["status"], "applied");
                }
                assert!(
                    request["messages"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|m| m.get("_memivy_request_reads").is_none())
                );
                text("已结束本轮处理。")
            }
            _ => suggestions(),
        })
    });

    let first = store
        .run_discussion(&config, &run.input_id, &run.attempt_id, "zh-CN", |_| {
            let saved = store.agent_execution(&run.input_id).unwrap();
            if saved.state == "processing"
                && saved.protocol.last().is_some_and(|m| {
                    checkpoint_calls(m)
                        .iter()
                        .any(|c| c.id.as_str() == "pending-write")
                })
            {
                store
                    .stop_agent_input(&run.input_id, &run.attempt_id, "failed", Some("network"))
                    .unwrap();
            }
        })
        .await;
    assert!(first.is_err());
    assert!(
        store
            .agent_input_receipts(&run.input_id)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store.memory(&captured.memory_id).unwrap().current.id,
        captured.version_id
    );
    drop(store);

    let reopened = MemoryStore::open(dir.path()).unwrap();
    let saved = reopened.agent_execution(&run.input_id).unwrap();
    assert!(
        saved.protocol[2]["content"][0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("背景。")
    );
    assert!(saved.protocol.last().unwrap()["_memivy_request_reads"].is_array());
    let retry = reopened.retry_agent_input(&run.input_id, &id()).unwrap();
    reopened
        .run_discussion(&config, &retry.input_id, &retry.attempt_id, "zh-CN", |_| {})
        .await
        .unwrap();
    server.join().unwrap();
    assert_eq!(requests.lock().unwrap().len(), 3);
    let current = reopened.memory(&captured.memory_id).unwrap().current;
    if prune {
        assert_eq!(current.id, captured.version_id);
        assert_eq!(current.body, original);
        assert!(
            reopened
                .agent_input_receipts(&run.input_id)
                .unwrap()
                .is_empty()
        );
    } else {
        assert_ne!(current.id, captured.version_id);
        assert_eq!(current.body, format!("{original}需要支持离线。"));
        assert_eq!(
            reopened.agent_input_receipts(&run.input_id).unwrap().len(),
            1
        );
    }
    reopened.check_integrity().unwrap();
}

#[tokio::test]
async fn released_body_cannot_authorize_a_pending_write_after_restart() {
    pending_write_rechecks_the_persisted_request_view(true).await;
}

#[tokio::test]
async fn fully_visible_body_authorizes_the_same_pending_write_after_restart() {
    pending_write_rechecks_the_persisted_request_view(false).await;
}

#[tokio::test]
async fn rereading_compacted_conversation_supplies_the_whole_input_undo_handle() {
    let (_dir, store, conversation) = setup();
    let first = begin(
        &store,
        &conversation,
        "想到做离线访谈标注工具，先帮我记好。",
        &[],
    );
    let original_id = first.input_id.clone();
    let source = first.user_message_id.clone();
    let source_quote = first.input_text.clone();
    let topic = conversation.clone();
    let (config, _, server) = fixture(7, move |index, request| {
        Response::stream(match index {
            0 => call(
                "create",
                "write_memory",
                json!({"destination":{"kind":"new"},"title":"离线访谈标注想法","parts":[{"text":"考虑做离线访谈标注工具，尚未决定或执行。","sources":[{"source_id":source,"quote":source_quote}]}]}),
            ),
            1 => text("已记下这个想法。"),
            2 | 6 => suggestions(),
            3 => call(
                "past",
                "read_conversation",
                json!({"conversation_id":topic,"after_seq":0,"limit":2}),
            ),
            4 => {
                let result = last_content(request);
                assert_eq!(result["messages"][0]["logical_input_id"], original_id);
                assert_eq!(result["messages"][1]["logical_input_id"], original_id);
                call(
                    "reverse",
                    "undo_changes",
                    json!({"input_id":result["messages"][0]["logical_input_id"]}),
                )
            }
            5 => {
                assert_eq!(last_content(request)["receipt"]["status"], "applied");
                text("最开始那次记忆修改已经撤销，原话仍保留。")
            }
            _ => unreachable!(),
        })
    });

    store
        .run_discussion(&config, &first.input_id, &first.attempt_id, "zh-CN", |_| {})
        .await
        .unwrap();
    let next = begin(&store, &conversation, "请撤销最开始那次修改。", &[]);
    let context = store.agent_conversation_context(&conversation).unwrap();
    store
        .save_agent_summary(
            &next.input_id,
            &next.attempt_id,
            "曾讨论一个离线工具想法。",
            store.turn(&first.input_id).unwrap().assistant.seq,
            &context.receipt_revision,
        )
        .unwrap();
    store
        .run_discussion(&config, &next.input_id, &next.attempt_id, "zh-CN", |_| {})
        .await
        .unwrap();
    server.join().unwrap();
    assert_eq!(
        store.agent_input_receipts(&first.input_id).unwrap()[0].status,
        "undone"
    );
    assert!(
        store
            .library(&LibraryQuery::default())
            .unwrap()
            .items
            .is_empty()
    );
    assert_eq!(count(&store, "captures"), 1);
}

#[tokio::test]
async fn legacy_control_checkpoint_retries_with_current_prompt_and_preserves_history() {
    let (_dir, store, conversation) = setup();
    let run = begin(&store, &conversation, "请继续分析。", &[]);
    let old = vec![
        json!({"role":"system","content":"Always call set_turn_options before answering."}),
        json!({"role":"user","content":run.input_text}),
        json!({"role":"assistant","content":null,"tool_calls":[{"id":"old-options","type":"function","function":{"name":"set_turn_options","arguments":"{\"reply_kind\":\"acknowledgment_only\",\"maintenance\":\"unchanged\"}"}}]}),
    ];
    store
        .checkpoint_agent(&run.input_id, &run.attempt_id, &old)
        .unwrap();
    store
        .stop_agent_input(&run.input_id, &run.attempt_id, "failed", Some("network"))
        .unwrap();
    let retry = store.retry_agent_input(&run.input_id, &id()).unwrap();
    let (config, requests, server) = fixture(2, |index, request| {
        Response::stream(if index == 0 {
            assert!(
                !request["messages"][0]
                    .to_string()
                    .contains("set_turn_options")
            );
            assert!(
                request["messages"][0]
                    .to_string()
                    .contains("Respond naturally")
            );
            assert_eq!(last_content(request)["applied"], false);
            text("可以继续比较成本和时间。")
        } else {
            suggestions()
        })
    });
    store
        .run_discussion(&config, &retry.input_id, &retry.attempt_id, "zh-CN", |_| {})
        .await
        .unwrap();
    server.join().unwrap();
    let result = store.agent_execution(&run.input_id).unwrap();
    assert_eq!(result.state, "complete");
    assert_eq!(result.protocol[0], old[0]);
    assert_eq!(result.follow_ups.len(), 2);
    assert_eq!(requests.lock().unwrap().len(), 2);
    assert_eq!(count(&store, "memories"), 0);
}
