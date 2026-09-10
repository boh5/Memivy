use memivy_core::{
    memory::*,
    model::{ModelConfig, ProbeError},
};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
    time::Duration,
};
fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
type Requests = Arc<Mutex<Vec<Value>>>;
fn fixture(
    count: usize,
    mut respond: impl FnMut(usize, &Value) -> (u16, Value) + Send + 'static,
) -> (ModelConfig, Requests, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let config = ModelConfig {
        base_url: format!("http://{}/v1", listener.local_addr().unwrap()),
        model: "synthetic-agent".into(),
        api_key: None,
        max_output_tokens: None,
        output_token_parameter: Default::default(),
        disable_reasoning: false,
    };
    let requests = Arc::new(Mutex::new(vec![]));
    let log = requests.clone();
    let server = std::thread::spawn(move || {
        for n in 0..count {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut bytes = vec![];
            let end = loop {
                let mut part = [0; 4096];
                let len = socket.read(&mut part).unwrap();
                assert!(len > 0);
                bytes.extend_from_slice(&part[..len]);
                if let Some(at) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                    break at + 4;
                }
            };
            let size = String::from_utf8_lossy(&bytes[..end])
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            while bytes.len() < end + size {
                let mut part = [0; 4096];
                let len = socket.read(&mut part).unwrap();
                assert!(len > 0);
                bytes.extend_from_slice(&part[..len]);
            }
            let request: Value = serde_json::from_slice(&bytes[end..end + size]).unwrap();
            log.lock().unwrap().push(request.clone());
            let (status, value) = respond(n, &request);
            let body = value.to_string();
            write!(socket,"HTTP/1.1 {status} QA\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
    });
    (config, requests, server)
}
fn tool(n: usize, name: &str, args: Value) -> Value {
    json!({"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","content":null,"reasoning_content":"synthetic protocol field","tool_calls":[{"id":format!("call-{n}"),"type":"function","function":{"name":name,"arguments":args.to_string()}}]}}]})
}
fn content(value: Value) -> Value {
    json!({"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":value.to_string()}}]})
}
fn probe(n: usize) -> Value {
    match n {
        0 => content(json!({"ok":true,"echo":"先留住原话"})),
        1 => tool(n, "probe_lookup", json!({"key":"synthetic"})),
        2 => tool(n, "probe_finish", json!({"value":"MEMIVY-PROTOCOL-47"})),
        _ => unreachable!(),
    }
}
fn capture(store: &MemoryStore, text: &str) -> CaptureResult {
    store
        .capture(&CaptureRequest {
            request_id: id(),
            text: text.into(),
            origin: Origin::User {
                app: "QA".into(),
                project: None,
                uri: None,
            },
        })
        .unwrap()
}
fn topic(store: &MemoryStore) -> String {
    let topic = id();
    store.create_conversation(&topic, "Agent QA").unwrap();
    topic
}

#[tokio::test]
async fn agent_reads_disjoint_current_windows_and_preserves_protocol_without_saving_answer() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let text = format!(
        "anchor start-fact\n{}\nEND-4792",
        "ordinary background ".repeat(400)
    );
    let tail = text.chars().count() - 8;
    let saved = capture(&store, &text);
    let conversation = topic(&store);
    let (config, requests, server) = fixture(6, move |n, _| {
        (
            200,
            if n < 3 {
                probe(n)
            } else {
                match n {
                    3 => tool(
                        n,
                        "search_memories",
                        json!({"query":"anchor","variants":[],"limit":4}),
                    ),
                    4 => tool(
                        n,
                        "read_memory",
                        json!({"id":"M1","start_char":tail,"max_chars":1800}),
                    ),
                    _ => tool(
                        n,
                        "answer",
                        json!({"recollections":[{"text":"start-fact and END-4792","sources":["M1"]}],"ideas":"new idea only","conclusion":"review before save"}),
                    ),
                }
            },
        )
    });
    assert!(
        store
            .test_model_capabilities(&config)
            .await
            .unwrap()
            .multi_turn
    );
    let turn = store
        .start_turn(&id(), &conversation, "anchor detail", &[])
        .unwrap();
    store
        .answer_discussion(&config, &conversation, &turn, &[])
        .await
        .unwrap();
    server.join().unwrap();
    let result = store.turn(&turn.id).unwrap();
    assert_eq!(result.assistant.status, "complete");
    let source = SourceRef::Version(saved.version_id);
    let evidence = store
        .discussion_excerpt(&result.assistant.id, &source)
        .unwrap();
    assert!(evidence.text.contains("start-fact"));
    assert_eq!(evidence.additional_spans.len(), 1);
    assert_eq!(evidence.additional_spans[0].text, "END-4792");
    assert_eq!(store.memories(false, 100).unwrap().len(), 1);
    let requests = requests.lock().unwrap();
    let messages = requests[4]["messages"].as_array().unwrap();
    assert!(
        messages
            .iter()
            .any(|m| m["role"] == "assistant"
                && m["reasoning_content"] == "synthetic protocol field")
    );
    assert!(
        messages
            .iter()
            .any(|m| m["role"] == "tool" && m["tool_call_id"] == "call-3")
    );
    assert!(
        !requests[3..]
            .iter()
            .any(|r| r.to_string().contains("unread archive"))
    );
}

#[tokio::test]
async fn missing_multi_turn_uses_fixed_rag_and_capability_cache_follows_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    capture(&store, "anchor fact");
    let conversation = topic(&store);
    let (config, requests, server) = fixture(5, |n, _| match n {
        0 | 1 => (200, probe(n)),
        2 => (
            400,
            json!({"error":"synthetic unsupported tool-result messages"}),
        ),
        3 => (200, content(json!({"queries":["anchor"]}))),
        _ => (
            200,
            content(
                json!({"recollections":[{"text":"fact","sources":["M1"]}],"ideas":"","conclusion":""}),
            ),
        ),
    });
    let cap = store.test_model_capabilities(&config).await.unwrap();
    assert!(cap.structured_json && cap.single_tool && !cap.multi_turn);
    let turn = store
        .start_turn(&id(), &conversation, "anchor", &[])
        .unwrap();
    store
        .answer_discussion(&config, &conversation, &turn, &[])
        .await
        .unwrap();
    server.join().unwrap();
    assert!(requests.lock().unwrap()[3]["tools"].is_null());
    let mut changed = config.clone();
    changed.model = "different".into();
    assert!(store.model_capabilities(&changed).is_none());
}

#[tokio::test]
async fn missing_single_tool_pauses_organization_and_auth_failure_is_not_cached() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    capture(&store, "immediately available");
    let (config, _, server) = fixture(2, |n, _| {
        if n == 0 {
            (200, probe(n))
        } else {
            (400, json!({"error":"tools unsupported"}))
        }
    });
    assert!(
        !store
            .test_model_capabilities(&config)
            .await
            .unwrap()
            .single_tool
    );
    server.join().unwrap();
    let mut task = store.claim_organization().unwrap().unwrap();
    store.prepare_organization(&mut task).unwrap();
    assert_eq!(
        store
            .propose_organization_flow(&config, &mut task)
            .await
            .unwrap_err(),
        ProbeError::ToolsUnsupported
    );
    store
        .fail_organization(&task.attempt_id, "tools_unsupported")
        .unwrap();
    assert_eq!(store.memories(false, 10).unwrap().len(), 1);
    let (other, _, server) = fixture(1, |_, _| (401, json!({"error":"synthetic unauthorized"})));
    assert_eq!(
        store.test_model_capabilities(&other).await.unwrap_err(),
        ProbeError::Status(401)
    );
    server.join().unwrap();
    assert!(store.model_capabilities(&other).is_none());
}

#[tokio::test]
async fn ingestion_can_search_a_target_then_apply_one_reversible_merge() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let target = capture(&store, "anchor project remains uncertain");
    store
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: target.memory_id.clone(),
            expected_version: target.version_id,
            title: "anchor".into(),
            body: "anchor project remains uncertain".into(),
        })
        .unwrap();
    let incoming = capture(&store, "new project observation");
    let mut task = store.claim_organization().unwrap().unwrap();
    assert_eq!(task.memory.memory_id, incoming.memory_id);
    let (config, _, server) = fixture(5, |n, _| {
        (
            200,
            if n < 3 {
                probe(n)
            } else if n == 3 {
                tool(
                    n,
                    "search_memories",
                    json!({"query":"anchor","variants":[],"limit":4}),
                )
            } else {
                tool(
                    n,
                    "merge_memory",
                    json!({"target":"M1","addition":"new project observation","changes":[],"keywords":[],"reason":"same explicit project"}),
                )
            },
        )
    });
    store.test_model_capabilities(&config).await.unwrap();
    let proposal = store
        .propose_organization_flow(&config, &mut task)
        .await
        .unwrap();
    server.join().unwrap();
    assert_eq!(store.memories(false, 10).unwrap().len(), 2);
    let receipt = store.apply_organization(&task, &proposal).unwrap();
    assert_eq!(receipt.status, "applied");
    assert!(
        store
            .memory(&target.memory_id)
            .unwrap()
            .current
            .body
            .contains("new project observation")
    );
    assert_eq!(store.memories(false, 10).unwrap().len(), 1);
    store.undo(&id(), &receipt.request_id).unwrap();
    assert_eq!(store.memories(false, 10).unwrap().len(), 2);
}

#[tokio::test]
async fn unknown_tool_and_changed_evidence_cannot_complete_answers() {
    for changed in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let saved = capture(&store, "anchor old");
        let conversation = topic(&store);
        let writer = store.clone();
        let (config, _, server) = fixture(if changed { 5 } else { 4 }, move |n, _| {
            (
                200,
                if n < 3 {
                    probe(n)
                } else if !changed {
                    tool(n, "read_archive", json!({"id":"anything"}))
                } else if n == 3 {
                    tool(
                        n,
                        "search_memories",
                        json!({"query":"anchor","variants":[],"limit":4}),
                    )
                } else {
                    writer
                        .edit_memory(&EditRequest {
                            request_id: id(),
                            memory_id: saved.memory_id.clone(),
                            expected_version: saved.version_id.clone(),
                            title: "changed".into(),
                            body: "current changed".into(),
                        })
                        .unwrap();
                    tool(
                        n,
                        "answer",
                        json!({"recollections":[{"text":"old","sources":["M1"]}],"ideas":"","conclusion":""}),
                    )
                },
            )
        });
        store.test_model_capabilities(&config).await.unwrap();
        let turn = store
            .start_turn(&id(), &conversation, "anchor", &[])
            .unwrap();
        assert!(
            store
                .answer_discussion(&config, &conversation, &turn, &[])
                .await
                .is_err()
        );
        server.join().unwrap();
        assert_ne!(store.turn(&turn.id).unwrap().assistant.status, "complete");
        assert_eq!(store.memories(false, 10).unwrap().len(), 1);
    }
}

#[tokio::test]
async fn first_miss_can_rephrase_and_repeated_read_ends_evidence_rounds() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    capture(&store, "anchor actual fact");
    let conversation = topic(&store);
    let (config, requests, server) = fixture(7, |n, request| {
        (
            200,
            if n < 3 {
                probe(n)
            } else {
                match n {
                    3 => tool(
                        n,
                        "search_memories",
                        json!({"query":"nothing-matches","variants":[],"limit":4}),
                    ),
                    4 => tool(
                        n,
                        "search_memories",
                        json!({"query":"anchor","variants":[],"limit":4}),
                    ),
                    5 => tool(
                        n,
                        "read_memory",
                        json!({"id":"M1","start_char":0,"max_chars":1800}),
                    ),
                    _ => {
                        assert!(
                            request["tools"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .all(|t| t["function"]["name"] == "answer")
                        );
                        tool(
                            n,
                            "answer",
                            json!({"recollections":[{"text":"actual fact","sources":["M1"]}],"ideas":"","conclusion":""}),
                        )
                    }
                }
            },
        )
    });
    store.test_model_capabilities(&config).await.unwrap();
    let turn = store
        .start_turn(&id(), &conversation, "find my fact", &[])
        .unwrap();
    store
        .answer_discussion(&config, &conversation, &turn, &[])
        .await
        .unwrap();
    server.join().unwrap();
    assert_eq!(requests.lock().unwrap().len(), 7);
}

#[tokio::test]
async fn ingestion_keeps_multibyte_body_within_character_limit() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let body = "背景说明。".repeat(1220);
    let incoming = capture(&store, &body);
    let mut task = store.claim_organization().unwrap().unwrap();
    let expected = body.clone();
    let (config, _, server) = fixture(4, move |n, _| {
        (
            200,
            if n < 3 {
                probe(n)
            } else {
                tool(
                    n,
                    "keep_memory",
                    json!({"title":"长中文记录","body":expected,"keywords":[],"reason":"保留完整正文"}),
                )
            },
        )
    });
    store.test_model_capabilities(&config).await.unwrap();
    let proposal = store
        .propose_organization_flow(&config, &mut task)
        .await
        .unwrap();
    server.join().unwrap();
    store.apply_organization(&task, &proposal).unwrap();
    assert_eq!(
        store.memory(&incoming.memory_id).unwrap().current.body,
        body
    );
}
