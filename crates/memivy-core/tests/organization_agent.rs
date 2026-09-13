use memivy_core::{
    memory::*,
    model::{ModelConfig, ProbeError, tools},
};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};

fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn capture(store: &MemoryStore, text: &str) -> CaptureResult {
    store
        .capture(&CaptureRequest {
            request_id: id(),
            text: text.into(),
            origin: Origin::User {
                app: "synthetic agent test".into(),
                project: None,
                uri: None,
            },
        })
        .unwrap()
}
fn fixture(
    count: usize,
    response: impl Fn(usize, &Value) -> String + Send + 'static,
) -> (ModelConfig, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let config = ModelConfig {
        base_url: format!("http://{}/v1", listener.local_addr().unwrap()),
        model: "synthetic".into(),
        api_key: None,
        max_output_tokens: None,
        output_token_parameter: Default::default(),
        disable_reasoning: false,
    };
    let handle = std::thread::spawn(move || {
        for index in 0..count {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut header = vec![];
            while !header.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                socket.read_exact(&mut byte).unwrap();
                header.push(byte[0]);
            }
            let size: usize = String::from_utf8(header)
                .unwrap()
                .lines()
                .find_map(|line| {
                    line.to_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse().unwrap())
                })
                .unwrap();
            let mut body = vec![0; size];
            socket.read_exact(&mut body).unwrap();
            let request: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(request["stream"], true);
            assert_eq!(request["tool_choice"], "auto");
            let result = response(index, &request);
            if result.is_empty() {
                continue;
            }
            write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{result}",result.len()).unwrap();
        }
    });
    (config, handle)
}
fn ready(store: &MemoryStore, config: &ModelConfig) {
    tools::save_capabilities(
        store.database_path().parent().unwrap(),
        config,
        &tools::Capabilities {
            structured_json: true,
            streaming_text: true,
            single_tool: true,
            multi_turn: true,
        },
    )
    .unwrap();
}
fn event(delta: Value, finish: &str) -> String {
    format!(
        "data: {}\n\ndata: [DONE]\n\n",
        json!({"choices":[{"index":0,"delta":delta,"finish_reason":finish}]})
    )
}
fn write_call(call_id: &str, change: &MemoryWriteArgs) -> String {
    event(
        json!({"role":"assistant","tool_calls":[{"index":0,"id":call_id,"type":"function","function":{"name":"write_memory","arguments":serde_json::to_string(change).unwrap()}}]}),
        "tool_calls",
    )
}
fn quoted_part(text: impl Into<String>, source_id: &str, quote: &str) -> MemoryWritePart {
    MemoryWritePart {
        text: text.into(),
        sources: vec![MemorySourceQuote {
            source_id: source_id.into(),
            quote: quote.into(),
        }],
    }
}
fn final_text() -> String {
    event(json!({"role":"assistant","content":"已记下。"}), "stop")
}

#[tokio::test]
async fn capture_uses_natural_agent_and_one_existing_memory_identity() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let raw = capture(&store, "我计划上线，但尚未执行。");
    let mut task = store.claim_organization().unwrap().unwrap();
    store.prepare_organization(&mut task).unwrap();
    let source = raw.capture_id.clone();
    let (config, server) = fixture(2, move |index, request| {
        if index == 0 {
            write_call(
                "write-1",
                &MemoryWriteArgs {
                    destination: Destination::New,
                    title: "上线计划".into(),
                    parts: vec![quoted_part(
                        "计划上线，尚未执行。",
                        &source,
                        "我计划上线，但尚未执行。",
                    )],
                },
            )
        } else {
            let result: Value = serde_json::from_str(
                request["messages"].as_array().unwrap().last().unwrap()["content"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(result["receipt"]["status"], "applied");
            final_text()
        }
    });
    ready(&store, &config);
    let receipt = store
        .run_organization(&config, &task)
        .await
        .unwrap()
        .unwrap();
    server.join().unwrap();
    assert_eq!(receipt.memory_id.as_deref(), Some(raw.memory_id.as_str()));
    assert_eq!(
        store.library(&LibraryQuery::default()).unwrap().items.len(),
        1
    );
    assert_eq!(
        store.capture_by_id(&raw.capture_id).unwrap().text,
        "我计划上线，但尚未执行。"
    );
    assert_eq!(
        store.memory(&raw.memory_id).unwrap().current.body,
        "计划上线，尚未执行。"
    );
    assert!(store.claim_organization().unwrap().is_none());
    store.check_integrity().unwrap();
}

#[tokio::test]
async fn natural_unchanged_completion_does_not_create_a_false_mutation_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let raw = capture(&store, "那个以后再说。");
    let task = store.claim_organization().unwrap().unwrap();
    let (config, server) = fixture(1, |_, _| final_text());
    ready(&store, &config);
    assert!(
        store
            .run_organization(&config, &task)
            .await
            .unwrap()
            .is_none()
    );
    server.join().unwrap();
    assert_eq!(
        store.memory(&raw.memory_id).unwrap().current.id,
        raw.version_id
    );
    let jobs = store
        .organization_jobs(&RecordKey {
            kind: "memory".into(),
            id: raw.memory_id,
        })
        .unwrap();
    assert_eq!(jobs[0].status, "done");
    assert!(jobs[0].receipt.is_none());
    assert!(!jobs[0].can_retry);
}

#[tokio::test]
async fn target_read_and_continuation_preserve_sources_and_reversible_merge() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let target = capture(&store, "连接池 4 个连接。保留只读查询。");
    store
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: target.memory_id.clone(),
            expected_version: target.version_id,
            title: "数据库".into(),
            body: "连接池 4 个连接。保留只读查询。".into(),
        })
        .unwrap();
    let previous = store.memory(&target.memory_id).unwrap().current;
    let input = capture(&store, "决定连接池改为 8 个连接，尚未执行。");
    let task = store.claim_organization().unwrap().unwrap();
    let target_id = target.memory_id.clone();
    let source = input.capture_id.clone();
    let version = previous.id.clone();
    let (config, server) = fixture(3, move |index, request| match index {
        0 => event(
            json!({"role":"assistant","tool_calls":[{"index":0,"id":"read-1","type":"function","function":{"name":"read_memory","arguments":json!({"memory_id":target_id,"view":"current","source_id":null,"start_char":0,"max_chars":3000}).to_string()}}]}),
            "tool_calls",
        ),
        1 => {
            let result: Value = serde_json::from_str(
                request["messages"].as_array().unwrap().last().unwrap()["content"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(
                result["evidence"]["text"],
                "连接池 4 个连接。保留只读查询。"
            );
            write_call(
                "write-1",
                &MemoryWriteArgs {
                    destination: Destination::Existing {
                        memory_id: target_id.clone(),
                        expected_version: version.clone(),
                    },
                    title: "数据库".into(),
                    parts: vec![
                        quoted_part("原来连接池 4 个连接。", &version, "连接池 4 个连接。"),
                        quoted_part(
                            "现在决定改为 8 个连接，尚未执行。",
                            &source,
                            "决定连接池改为 8 个连接，尚未执行。",
                        ),
                        quoted_part("保留只读查询。", &version, "保留只读查询。"),
                    ],
                },
            )
        }
        _ => final_text(),
    });
    ready(&store, &config);
    let receipt = store
        .run_organization(&config, &task)
        .await
        .unwrap()
        .unwrap();
    server.join().unwrap();
    let current = store.memory(&target.memory_id).unwrap().current;
    assert_eq!(current.capture_ids.len(), 2);
    assert!(current.capture_ids.contains(&input.capture_id));
    assert_eq!(
        store.library(&LibraryQuery::default()).unwrap().items.len(),
        1
    );
    store.undo(&id(), &receipt.request_id).unwrap();
    assert_eq!(
        store.memory(&target.memory_id).unwrap().current.body,
        previous.body
    );
    assert_eq!(
        store.memory(&input.memory_id).unwrap().current.body,
        "决定连接池改为 8 个连接，尚未执行。"
    );
    store.check_integrity().unwrap();
}

#[tokio::test]
async fn committed_write_survives_failed_final_acknowledgement_without_requeueing() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let raw = capture(&store, "独立想法");
    let task = store.claim_organization().unwrap().unwrap();
    let source = raw.capture_id.clone();
    let (config, server) = fixture(2, move |index, _| {
        if index == 0 {
            write_call(
                "write-1",
                &MemoryWriteArgs {
                    destination: Destination::New,
                    title: "想法".into(),
                    parts: vec![quoted_part("独立想法，待考虑。", &source, "独立想法")],
                },
            )
        } else {
            String::new()
        }
    });
    ready(&store, &config);
    let receipt = store
        .run_organization(&config, &task)
        .await
        .unwrap()
        .unwrap();
    server.join().unwrap();
    assert_eq!(receipt.status, "applied");
    store.recover_organization().unwrap();
    assert!(store.claim_organization().unwrap().is_none());
    assert_eq!(
        store.memory(&raw.memory_id).unwrap().current.body,
        "独立想法，待考虑。"
    );
}

#[tokio::test]
async fn unsupported_capability_keeps_capture_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let raw = capture(&store, "保留原话");
    let task = store.claim_organization().unwrap().unwrap();
    let config = ModelConfig {
        base_url: "http://127.0.0.1:9/v1".into(),
        model: "unverified".into(),
        api_key: None,
        max_output_tokens: None,
        output_token_parameter: Default::default(),
        disable_reasoning: false,
    };
    assert_eq!(
        store.run_organization(&config, &task).await.unwrap_err(),
        ProbeError::ToolsUnsupported
    );
    assert_eq!(
        store.memory(&raw.memory_id).unwrap().current.id,
        raw.version_id
    );
}

#[tokio::test]
async fn candidate_excerpt_cannot_replace_an_unread_tail() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let original = format!(
        "连接池 4 个连接。{}尾部条件：只读查询必须保留。",
        "连接池背景。".repeat(600)
    );
    let target = capture(&store, &original);
    store
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: target.memory_id.clone(),
            expected_version: target.version_id,
            title: "连接池".into(),
            body: original.clone(),
        })
        .unwrap();
    let before = store.memory(&target.memory_id).unwrap().current;
    let input = capture(&store, "决定把连接池改成 8 个连接。");
    let mut task = store.claim_organization().unwrap().unwrap();
    store.prepare_organization(&mut task).unwrap();
    let candidate = task
        .candidates
        .iter()
        .find(|hit| hit.memory_id == target.memory_id)
        .unwrap();
    assert!(candidate.evidence.text.chars().count() < original.chars().count());
    assert!(!candidate.evidence.text.contains("尾部条件"));
    let target_id = target.memory_id.clone();
    let version = before.id.clone();
    let source = input.capture_id.clone();
    let (config, server) = fixture(2, move |index, request| {
        if index == 0 {
            assert!(!request.to_string().contains("尾部条件"));
            write_call(
                "unread-tail",
                &MemoryWriteArgs {
                    destination: Destination::Existing {
                        memory_id: target_id.clone(),
                        expected_version: version.clone(),
                    },
                    title: "连接池".into(),
                    parts: vec![quoted_part(
                        "连接池改成 8 个连接。",
                        &source,
                        "决定把连接池改成 8 个连接。",
                    )],
                },
            )
        } else {
            let result: Value = serde_json::from_str(
                request["messages"].as_array().unwrap().last().unwrap()["content"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(result["applied"], false);
            assert!(result["error"].as_str().unwrap().contains("not visible"));
            event(
                json!({"role":"assistant","content":"缺少完整正文，本次没有修改记忆。"}),
                "stop",
            )
        }
    });
    ready(&store, &config);
    assert!(
        store
            .run_organization(&config, &task)
            .await
            .unwrap()
            .is_none()
    );
    server.join().unwrap();
    let after = store.memory(&target.memory_id).unwrap().current;
    assert_eq!(after.id, before.id);
    assert_eq!(after.body, original);
    assert_eq!(
        store.memory(&input.memory_id).unwrap().current.id,
        input.version_id
    );
    assert!(
        store
            .organization_jobs(&RecordKey {
                kind: "memory".into(),
                id: input.memory_id
            })
            .unwrap()[0]
            .receipt
            .is_none()
    );
    store.check_integrity().unwrap();
}

#[tokio::test]
async fn paged_target_reads_allow_a_complete_body_update_preserving_the_tail() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let original = format!("{}尾部条件：只读查询必须保留。", "连接池背景。".repeat(600));
    let target = capture(&store, &original);
    store
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: target.memory_id.clone(),
            expected_version: target.version_id,
            title: "连接池".into(),
            body: original.clone(),
        })
        .unwrap();
    let before = store.memory(&target.memory_id).unwrap().current;
    let input = capture(&store, "决定连接池为 8 个连接。");
    let task = store.claim_organization().unwrap().unwrap();
    let target_id = target.memory_id.clone();
    let version = before.id;
    let source = input.capture_id;
    let revised = format!("{original}决定连接池为 8 个连接。");
    let expected = revised.clone();
    let (config, server) = fixture(4, move |index, request| match index {
        0 | 1 => event(
            json!({"role":"assistant","tool_calls":[{"index":0,"id":format!("read-{index}"),"type":"function","function":{
                "name":"read_memory","arguments":json!({"memory_id":target_id,"view":"current","source_id":null,"start_char":index*3000,"max_chars":3000}).to_string()
            }}]}),
            "tool_calls",
        ),
        2 => {
            assert!(request.to_string().contains("尾部条件"));
            write_call(
                "complete-read",
                &MemoryWriteArgs {
                    destination: Destination::Existing {
                        memory_id: target_id.clone(),
                        expected_version: version.clone(),
                    },
                    title: "连接池".into(),
                    parts: vec![
                        MemoryWritePart {
                            text: original.clone(),
                            sources: vec![],
                        },
                        quoted_part(
                            "决定连接池为 8 个连接。",
                            &source,
                            "决定连接池为 8 个连接。",
                        ),
                    ],
                },
            )
        }
        _ => final_text(),
    });
    ready(&store, &config);
    let receipt = store
        .run_organization(&config, &task)
        .await
        .unwrap()
        .unwrap();
    server.join().unwrap();
    assert_eq!(receipt.status, "applied");
    assert_eq!(
        store.memory(&target.memory_id).unwrap().current.body,
        expected
    );
    store.check_integrity().unwrap();
}

#[tokio::test]
async fn trash_between_requests_removes_candidate_body_and_extra_spans_before_writing() {
    let dir = tempfile::tempdir().unwrap();
    let store = std::sync::Arc::new(MemoryStore::open(dir.path()).unwrap());
    let primary = "候选主片段：不得上传声音。";
    let extra = "候选额外片段：限定只在本机处理。";
    let body = format!("{primary}{extra}");
    let target = capture(&store, &body);
    store
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: target.memory_id.clone(),
            expected_version: target.version_id,
            title: "语音规则".into(),
            body: body.clone(),
        })
        .unwrap();
    let before = store.memory(&target.memory_id).unwrap().current;
    let input = capture(&store, "语音规则补充：需要离线使用。");
    let mut task = store.claim_organization().unwrap().unwrap();
    // A candidate already selected before the request, with two actual spans
    // from its persisted version. Retrieval ranking is independent of deletion.
    task.candidates = vec![SearchHit {
        memory_id: target.memory_id.clone(),
        version_id: before.id.clone(),
        title: before.title.clone(),
        origins: vec![],
        updated_at: before.created_at,
        score: 1.0,
        evidence: Evidence {
            source: SourceRef::Version(before.id.clone()),
            title: before.title.clone(),
            text: primary.into(),
            start: 0,
            recorded_at: before.created_at,
            current: true,
            truncated: true,
            additional_spans: vec![EvidenceSpan {
                start: primary.chars().count(),
                text: extra.into(),
                truncated: false,
            }],
        },
    }];
    let changing_store = store.clone();
    let target_id = target.memory_id.clone();
    let version = before.id.clone();
    let source = input.capture_id.clone();
    let (config, server) = fixture(3, move |index, request| {
        let wire = request.to_string();
        if index == 0 {
            assert!(wire.contains(primary));
            assert!(wire.contains(extra));
            // The first model request has already arrived. Trash through the
            // production transaction before the next tool/request can execute.
            changing_store.trash_memory(&target_id, &version).unwrap();
            event(
                json!({"role":"assistant","tool_calls":[{"index":0,"id":"read-deleted","type":"function","function":{
                    "name":"read_memory","arguments":json!({"memory_id":target_id,"view":"current","source_id":null,"start_char":0,"max_chars":3000}).to_string()
                }}]}),
                "tool_calls",
            )
        } else {
            assert!(!wire.contains(primary));
            assert!(!wire.contains(extra));
            let context: Value =
                serde_json::from_str(request["messages"][1]["content"].as_str().unwrap()).unwrap();
            let evidence = &context["related_memories"][0]["evidence"];
            assert_eq!(evidence["unavailable"], true);
            assert!(evidence.get("text").is_none());
            assert!(evidence.get("additional_spans").is_none());
            let result: Value = serde_json::from_str(
                request["messages"].as_array().unwrap().last().unwrap()["content"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
            assert!(result.get("error").is_some());
            if index == 1 {
                write_call(
                    "write-deleted",
                    &MemoryWriteArgs {
                        destination: Destination::Existing {
                            memory_id: target_id.clone(),
                            expected_version: version.clone(),
                        },
                        title: "语音规则".into(),
                        parts: vec![quoted_part(
                            "需要离线使用。",
                            &source,
                            "语音规则补充：需要离线使用。",
                        )],
                    },
                )
            } else {
                assert_eq!(result["applied"], false);
                assert!(result["error"].as_str().unwrap().contains("not visible"));
                event(
                    json!({"role":"assistant","content":"目标已删除，没有修改记忆。"}),
                    "stop",
                )
            }
        }
    });
    ready(&store, &config);
    assert!(
        store
            .run_organization(&config, &task)
            .await
            .unwrap()
            .is_none()
    );
    server.join().unwrap();
    assert!(matches!(
        store.memory(&target.memory_id),
        Err(DataError::Unavailable)
    ));
    let trashed = store
        .memories(true, 10)
        .unwrap()
        .into_iter()
        .find(|memory| memory.id == target.memory_id)
        .unwrap();
    assert_eq!(trashed.state, "trashed");
    assert_eq!(trashed.current.id, before.id);
    assert_eq!(trashed.current.body, body);
    assert_eq!(
        store.memory(&input.memory_id).unwrap().current.id,
        input.version_id
    );
    assert!(
        store
            .organization_jobs(&RecordKey {
                kind: "memory".into(),
                id: input.memory_id
            })
            .unwrap()[0]
            .receipt
            .is_none()
    );
    store.check_integrity().unwrap();
}

#[tokio::test]
async fn invalid_quote_is_rejected_then_corrected_without_an_intermediate_write() {
    let dir = tempfile::tempdir().unwrap();
    let store = std::sync::Arc::new(MemoryStore::open(dir.path()).unwrap());
    let raw = capture(&store, "我计划上线，但尚未执行。");
    let task = store.claim_organization().unwrap().unwrap();
    let source = raw.capture_id.clone();
    let unchanged = store.clone();
    let original_version = raw.version_id.clone();
    let memory = raw.memory_id.clone();
    let (config, server) = fixture(3, move |index, request| {
        if index == 2 {
            return final_text();
        }
        if index == 1 {
            let result: Value = serde_json::from_str(
                request["messages"].as_array().unwrap().last().unwrap()["content"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(result["applied"], false);
            assert!(result["error"].as_str().unwrap().contains("逐项引用"));
            assert_eq!(
                unchanged.memory(&memory).unwrap().current.id,
                original_version
            );
            assert_eq!(unchanged.history(&memory).unwrap().len(), 1);
        }
        write_call(
            &format!("write-{index}"),
            &MemoryWriteArgs {
                destination: Destination::New,
                title: "上线计划".into(),
                parts: vec![quoted_part(
                    "计划上线，尚未执行。",
                    &source,
                    if index == 0 {
                        "我已经上线"
                    } else {
                        "我计划上线，但尚未执行。"
                    },
                )],
            },
        )
    });
    ready(&store, &config);
    let receipt = store
        .run_organization(&config, &task)
        .await
        .unwrap()
        .unwrap();
    server.join().unwrap();
    assert_eq!(receipt.status, "applied");
    assert_eq!(store.history(&raw.memory_id).unwrap().len(), 2);
    assert_eq!(
        store.memory(&raw.memory_id).unwrap().current.capture_ids,
        vec![raw.capture_id]
    );
    assert_eq!(
        store.memory(&raw.memory_id).unwrap().current.body,
        "计划上线，尚未执行。"
    );
    store.check_integrity().unwrap();
}

#[test]
fn organization_only_accepts_its_capture_and_current_target_version_sources() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let target = capture(&store, "之前每周4小时。\n不允许上传录音。");
    store
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: target.memory_id.clone(),
            expected_version: target.version_id,
            title: "现有条件".into(),
            body: "之前每周4小时。\n不允许上传录音。\n".into(),
        })
        .unwrap();
    let previous = store.memory(&target.memory_id).unwrap().current;
    let raw = capture(&store, "现在每周8小时。");
    let task = store.claim_organization().unwrap().unwrap();
    let foreign = capture(&store, "其他人的约束。");
    let write = MemoryWriteArgs {
        destination: Destination::Existing {
            memory_id: target.memory_id.clone(),
            expected_version: previous.id.clone(),
        },
        title: "当前条件".into(),
        parts: vec![
            quoted_part("此前每周4小时。\n", &previous.id, "之前每周4小时。"),
            quoted_part("现在每周8小时。\n", &raw.capture_id, "现在每周8小时。"),
            MemoryWritePart {
                text: "不允许上传录音。\n".into(),
                sources: vec![],
            },
        ],
    };
    let mut wrong = write.clone();
    wrong.parts[1].sources[0] = MemorySourceQuote {
        source_id: foreign.capture_id,
        quote: "其他人的约束。".into(),
    };
    assert_eq!(
        store.apply_organization(&task, &wrong),
        Err(DataError::SourceAttribution)
    );
    let mut only_existing = write.clone();
    only_existing.parts.remove(1);
    assert_eq!(
        store.apply_organization(&task, &only_existing),
        Err(DataError::SourceAttribution)
    );
    assert_eq!(
        store.memory(&target.memory_id).unwrap().current.id,
        previous.id
    );
    let receipt = store.apply_organization(&task, &write).unwrap();
    let current = store.memory(&target.memory_id).unwrap().current;
    assert_eq!(current.capture_ids.len(), 2);
    assert!(current.capture_ids.contains(&raw.capture_id));
    assert!(current.capture_ids.contains(&target.capture_id));
    assert_eq!(store.apply_organization(&task, &write).unwrap(), receipt);
    store.undo(&id(), &receipt.request_id).unwrap();
    assert_eq!(
        store.memory(&target.memory_id).unwrap().current.body,
        previous.body
    );
    assert_eq!(
        store.memory(&raw.memory_id).unwrap().current.body,
        "现在每周8小时。"
    );
    store.check_integrity().unwrap();
}
