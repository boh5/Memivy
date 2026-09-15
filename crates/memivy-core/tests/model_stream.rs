use memivy_core::model::{
    CompletionResponse, Message, ModelConfig, ProbeError, ToolDefinition, assistant, calls, text,
    tools::{self, StreamDelta, function, stream_turn},
};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
    time::Duration,
};

struct Response {
    status: u16,
    content_type: &'static str,
    body: String,
    split: bool,
}
impl Response {
    fn stream(body: String) -> Self {
        Self {
            status: 200,
            content_type: "text/event-stream",
            body,
            split: false,
        }
    }
}
type Requests = Arc<Mutex<Vec<Value>>>;
fn server(
    count: usize,
    response: impl Fn(usize, &Value) -> Response + Send + 'static,
) -> (ModelConfig, Requests, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let config = ModelConfig {
        provider: Default::default(),
        base_url: format!("http://{}/v1", listener.local_addr().unwrap()),
        model: "synthetic".into(),
        api_key: Some("fixture-secret".into()),
        max_output_tokens: Some(4096),
        output_token_parameter: Default::default(),
        disable_reasoning: true,
    };
    let requests = Arc::new(Mutex::new(vec![]));
    let seen = requests.clone();
    let handle = std::thread::spawn(move || {
        for index in 0..count {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut bytes = Vec::new();
            while !bytes.ends_with(b"\r\n\r\n") {
                let mut b = [0];
                socket.read_exact(&mut b).unwrap();
                bytes.push(b[0]);
            }
            let length: usize = String::from_utf8(bytes)
                .unwrap()
                .lines()
                .find_map(|line| {
                    line.to_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse().unwrap())
                })
                .unwrap();
            let mut body = vec![0; length];
            socket.read_exact(&mut body).unwrap();
            let request: Value = serde_json::from_slice(&body).unwrap();
            seen.lock().unwrap().push(request.clone());
            let reply = response(index, &request);
            let header = format!(
                "HTTP/1.1 {} Test\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                reply.status,
                reply.content_type,
                reply.body.len()
            );
            if socket.write_all(header.as_bytes()).is_err() {
                continue;
            }
            if reply.split {
                // Deliberately split inside UTF-8 code points and JSON tokens.
                for part in reply.body.as_bytes().chunks(2) {
                    if socket.write_all(part).is_err() {
                        break;
                    }
                    std::thread::sleep(Duration::from_micros(100));
                }
            } else {
                let _ = socket.write_all(reply.body.as_bytes());
            }
        }
    });
    (config, requests, handle)
}
fn delta(value: Value, finish: Value) -> String {
    format!(
        "data: {}\r\n\r\n",
        json!({"choices":[{"index":0,"delta":value,"finish_reason":finish}]})
    )
}
fn stop(text: &str) -> String {
    delta(json!({"role":"assistant","content":text}), json!(null))
        + &delta(json!({}), json!("stop"))
        + "data: [DONE]\r\n\r\n"
}
fn call(id: &str, name: &str, arguments: &str) -> String {
    delta(
        json!({"role":"assistant","tool_calls":[{"index":0,"id":id,"type":"function","function":{"name":name,"arguments":arguments}}]}),
        json!(null),
    ) + &delta(json!({}), json!("tool_calls"))
        + "data: [DONE]\r\n\r\n"
}
fn offered() -> Vec<ToolDefinition> {
    vec![function(
        "read_memory",
        "Read synthetic content",
        json!({"id":{"type":"string"}}),
    )]
}

#[tokio::test]
async fn text_is_emitted_before_completion_across_utf8_sse_boundaries() {
    let (config, requests, handle) = server(1, |_, _| {
        let mut response = Response::stream(
            delta(json!({"role":"assistant","content":"先记"}), json!(null))
                + &delta(json!({"content":"下来。"}), json!(null))
                + &delta(json!({}), json!("stop"))
                + "data: [DONE]\r\n\r\n",
        );
        response.split = true;
        response
    });
    let mut chunks = vec![];
    let result = stream_turn(
        &config,
        &[Message::user("synthetic")],
        &offered(),
        |event| {
            if let StreamDelta::Text(t) = event {
                chunks.push(t.text);
            }
            Ok(())
        },
    )
    .await
    .unwrap();
    handle.join().unwrap();
    assert_eq!(chunks, vec!["先记".to_string(), "下来。".to_string()]);
    assert_eq!(text(&result), "先记下来。");
    assert!(calls(&result).next().is_none());
    let request = &requests.lock().unwrap()[0];
    assert_eq!(request["stream"], true);
    assert_eq!(request["tool_choice"], "auto");
    assert_eq!(request["max_tokens"], 4096);
    assert_eq!(request["reasoning_effort"], "none");
}

#[tokio::test]
async fn only_complete_tool_arguments_are_returned_and_protocol_can_be_restored() {
    let (config, _, handle) = server(1, |_, _| {
        Response::stream(
            delta(
                json!({"role":"assistant","reasoning_content":"internal ","tool_calls":[{"index":0,"id":"call-1","type":"function","function":{"name":"read_memory","arguments":"{\"id\":"}}]}),
                json!(null),
            ) + &delta(
                json!({"reasoning_content":"context","tool_calls":[{"index":0,"function":{"arguments":"\"中文\"}"}}]}),
                json!(null),
            ) + &delta(json!({}), json!("tool_calls"))
                + "data: [DONE]\r\n\r\n",
        )
    });
    let mut events = vec![];
    let result = stream_turn(
        &config,
        &[Message::user("synthetic")],
        &offered(),
        |delta| {
            events.push(delta);
            Ok(())
        },
    )
    .await
    .unwrap();
    handle.join().unwrap();
    let complete_calls: Vec<_> = calls(&result).collect();
    assert_eq!(complete_calls.len(), 1);
    assert_eq!(complete_calls[0].function.arguments, json!({"id":"中文"}));
    assert_eq!(complete_calls[0].id.as_str(), "call-1");
    assert!(text(&result).is_empty());
    assert!(
        events
            .iter()
            .any(|e| matches!(e, StreamDelta::ToolCall { .. }))
    );
    let encoded = serde_json::to_string(&result).unwrap();
    let restored: CompletionResponse = serde_json::from_str(&encoded).unwrap();
    assert_eq!(assistant(restored), assistant(result));
    assert!(
        encoded.contains("internal") && encoded.contains("context"),
        "{encoded}"
    );
}

#[tokio::test]
async fn partial_truncated_unknown_and_duplicate_calls_never_become_executable() {
    let duplicate = delta(
        json!({"tool_calls":[
            {"index":0,"id":"same","type":"function","function":{"name":"read_memory","arguments":"{}"}},
            {"index":1,"id":"same","type":"function","function":{"name":"read_memory","arguments":"{}"}}
        ]}),
        json!("tool_calls"),
    ) + "data: [DONE]\r\n\r\n";
    let partial_sibling = delta(
        json!({"tool_calls":[
            {"index":0,"id":"complete","type":"function","function":{"name":"read_memory","arguments":"{\"id\":\"valid\"}"}},
            {"index":1,"id":"partial","type":"function","function":{"name":"read_memory","arguments":"{"}}
        ]}),
        json!("tool_calls"),
    ) + "data: [DONE]\r\n\r\n";
    let evicted = delta(
        json!({"tool_calls":[{"index":0,"id":"partial","type":"function","function":{"name":"read_memory","arguments":"{"}}]}),
        Value::Null,
    ) + &delta(
        json!({"tool_calls":[{"index":0,"id":"replacement","type":"function","function":{"name":"read_memory","arguments":"{\"id\":\"valid\"}"}}]}),
        json!("tool_calls"),
    ) + "data: [DONE]\r\n\r\n";
    let cases = [
        (
            stop("partial").replace("data: [DONE]\r\n\r\n", ""),
            ProbeError::InvalidResponse,
        ),
        (
            delta(json!({"content":"partial"}), json!("length")) + "data: [DONE]\r\n\r\n",
            ProbeError::Truncated,
        ),
        (
            call("call-1", "read_memory", "{\"id\":"),
            ProbeError::InvalidResponse,
        ),
        (
            call("call-1", "unavailable_tool", "{}"),
            ProbeError::InvalidResponse,
        ),
        (
            call("call-1", "read_memory", "[]"),
            ProbeError::InvalidResponse,
        ),
        (
            call("partial", "read_memory", "{").replace(
                "\"finish_reason\":\"tool_calls\"",
                "\"finish_reason\":\"length\"",
            ),
            ProbeError::Truncated,
        ),
        (duplicate, ProbeError::InvalidResponse),
        (partial_sibling, ProbeError::InvalidResponse),
        (evicted, ProbeError::InvalidResponse),
        (stop(&"x".repeat(1_048_576)), ProbeError::TooLarge),
        (
            delta(json!({"content":"unexpected stop"}), json!("unknown")) + "data: [DONE]\r\n\r\n",
            ProbeError::InvalidResponse,
        ),
        ("data: [DONE]\r\n\r\n".into(), ProbeError::InvalidResponse),
    ];
    for (case, (body, expected)) in cases.into_iter().enumerate() {
        let (config, _, handle) = server(1, move |_, _| Response::stream(body.clone()));
        assert_eq!(
            stream_turn(&config, &[Message::user("synthetic")], &offered(), |_| Ok(
                ()
            ))
            .await
            .unwrap_err(),
            expected,
            "malformed stream case {case}"
        );
        handle.join().unwrap();
    }
}

#[tokio::test]
async fn callback_failure_aborts_without_a_completed_turn() {
    let (config, _, handle) = server(1, |_, _| {
        let mut response = Response::stream(stop("cancel now"));
        response.split = true;
        response
    });
    let mut calls = 0;
    let result = stream_turn(&config, &[Message::user("synthetic")], &[], |_| {
        calls += 1;
        Err(ProbeError::Network)
    })
    .await;
    assert_eq!(result.unwrap_err(), ProbeError::Network);
    assert_eq!(calls, 1);
    handle.join().unwrap();
}

#[tokio::test]
async fn unsupported_and_http_errors_are_explicit_and_do_not_leak_server_body() {
    for (status, expected) in [
        (200, ProbeError::ToolsUnsupported),
        (401, ProbeError::Status(401)),
    ] {
        let (config, _, handle) = server(1, move |_, _| Response {
            status,
            content_type: "application/json",
            body: "fixture-secret provider diagnostic".into(),
            split: false,
        });
        let error = stream_turn(&config, &[Message::user("synthetic")], &[], |_| Ok(()))
            .await
            .unwrap_err();
        assert_eq!(error, expected);
        assert!(!error.to_string().contains("fixture-secret"));
        handle.join().unwrap();
    }
}

#[tokio::test]
async fn capability_probe_exercises_natural_multiturn_streaming_without_activation() {
    let (config, requests, handle) = server(5, |index, request| {
        match index {
        0 => Response {status:200,content_type:"application/json",body:json!({"id":"fixture","model":"synthetic","choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"{\"ok\":true,\"echo\":\"先留住原话\"}"}}]}).to_string(),split:false},
        1 => Response::stream(stop("MEMIVY-STREAM-READY")),
        2 => Response::stream(call("read-1", "probe_lookup", "{\"key\":\"synthetic\"}")),
        3 => {
            let messages=request["messages"].as_array().unwrap();
            let value: Value=serde_json::from_str(messages.last().unwrap()["content"].as_str().unwrap()).unwrap();
            Response::stream(call("echo-1", "probe_echo", &value.to_string()))
        },
        4 => {
            let messages=request["messages"].as_array().unwrap();
            assert_eq!(messages[1]["tool_calls"][0]["id"], "read-1");
            assert_eq!(messages[3]["tool_calls"][0]["id"], "echo-1");
            let value: Value=serde_json::from_str(messages.last().unwrap()["content"].as_str().unwrap()).unwrap();
            Response::stream(stop(value["value"].as_str().unwrap()))
        }, _ => unreachable!()
    }
    });
    let tested = tools::probe(&config).await.unwrap();
    handle.join().unwrap();
    assert!(tested.supports_agent() && tested.structured_json && tested.single_tool);
    let requests = requests.lock().unwrap();
    assert!(requests[1..].iter().all(|r| r["stream"] == true));
    assert!(requests[2..].iter().all(|r| r["tool_choice"] == "auto"));
}

#[tokio::test]
async fn fragmented_chat_reasoning_survives_saved_tool_history_and_exact_wire_ids() {
    let (config, requests, server) = server(2, |index, request| {
        if index == 0 {
            Response::stream(
                delta(
                    json!({"reasoning":"Think ","reasoning_content":null,"reasoning_details":[
                        {"type":"reasoning.text","index":2,"id":"thought-2","format":"anthropic-claude-v1","text":"Think "}
                    ]}),
                    Value::Null,
                ) + &delta(
                    json!({"reasoning":"first","reasoning_details":[
                        {"type":"reasoning.text","index":2,"text":"first","signature":"signed-"}
                    ]}),
                    Value::Null,
                ) + &delta(
                    json!({"reasoning_details":[
                        {"type":"reasoning.text","index":2,"signature":"payload"},
                        {"type":"reasoning.encrypted","index":3,"id":"rs_3","format":"openai-responses-v1","data":"encrypted-payload"}
                    ]}),
                    Value::Null,
                ) + &call("wire-call", "read_memory", "{\"id\":\"synthetic\"}"),
            )
        } else {
            let message = &request["messages"][1];
            assert_eq!(message["reasoning"], "Think first");
            assert_eq!(message["reasoning_content"], Value::Null);
            assert_eq!(
                message["reasoning_details"],
                json!([
                    {"type":"reasoning.text","index":2,"id":"thought-2","format":"anthropic-claude-v1","text":"Think first","signature":"signed-payload"},
                    {"type":"reasoning.encrypted","index":3,"id":"rs_3","format":"openai-responses-v1","data":"encrypted-payload"}
                ])
            );
            assert_eq!(message["tool_calls"][0]["id"], "wire-call");
            assert_eq!(request["messages"][2]["tool_call_id"], "wire-call");
            assert!(!request.to_string().contains("_memivy_openai_reasoning"));
            Response::stream(stop("Done"))
        }
    });
    let mut messages = vec![Message::user("Read the synthetic memory")];
    let mut first = stream_turn(&config, &messages, &offered(), |_| Ok(()))
        .await
        .unwrap();
    // Correlation IDs and provider wire IDs need not be identical.
    let tool = first
        .choice
        .iter_mut()
        .find_map(|part| match part {
            memivy_core::model::AssistantContent::ToolCall(call) => Some(call),
            _ => None,
        })
        .unwrap();
    tool.id = rig_core::message::ToolCallId::new("local-correlation").unwrap();
    let result = memivy_core::model::tool_result(tool, &json!({"value":"synthetic"}));
    let saved = serde_json::to_string(&assistant(first)).unwrap();
    messages.push(serde_json::from_str(&saved).unwrap());
    messages.push(result);
    assert_eq!(
        text(
            &stream_turn(&config, &messages, &offered(), |_| Ok(()))
                .await
                .unwrap()
        ),
        "Done"
    );
    server.join().unwrap();
    assert_eq!(requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn conflicting_reasoning_block_metadata_rejects_the_completed_tool_turn() {
    let (config, _, server) = server(1, |_, _| {
        Response::stream(
            delta(
                json!({"reasoning_details":[{"type":"reasoning.text","index":0,"id":"first","format":"format-v1","text":"a"}]}),
                Value::Null,
            ) + &delta(
                json!({"reasoning_details":[{"type":"reasoning.text","index":0,"id":"first","format":"conflicting-format","text":"b"}]}),
                Value::Null,
            ) + &call("wire-call", "read_memory", "{\"id\":\"synthetic\"}"),
        )
    });
    let result = stream_turn(&config, &[Message::user("Synthetic")], &offered(), |_| {
        Ok(())
    })
    .await;
    server.join().unwrap();
    assert_eq!(result.unwrap_err(), ProbeError::InvalidResponse);
}

#[tokio::test]
async fn injected_reasoning_counts_toward_the_actual_request_size_limit() {
    let (config, requests, server) =
        server(0, |_, _| unreachable!("oversized request must not connect"));
    let call = rig_core::message::ToolCall::from_wire("wire-call", rig_core::message::ToolFunction::new("read_memory".into(), json!({"id":"synthetic"})))
        .with_additional_params(Some(json!({"_memivy_openai_reasoning":{"reasoning_details":[{"type":"reasoning.encrypted","data":"x".repeat(1_048_576)}]}})));
    let result = memivy_core::model::tool_result(&call, &json!({"value":"synthetic"}));
    let messages = vec![
        Message::user("Synthetic"),
        Message::Assistant {
            id: None,
            content: vec![memivy_core::model::AssistantContent::ToolCall(call)],
        },
        result,
    ];
    assert_eq!(
        stream_turn(&config, &messages, &offered(), |_| Ok(()))
            .await
            .unwrap_err(),
        ProbeError::TooLarge
    );
    server.join().unwrap();
    assert!(requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn reused_wire_call_ids_keep_reasoning_on_the_correct_assistant_occurrence() {
    let (config, _, server) = server(1, |_, request| {
        assert!(request["messages"][1].get("reasoning_details").is_none());
        assert_eq!(
            request["messages"][3]["reasoning_details"],
            json!([
                {"type":"reasoning.encrypted","id":"later-thought","format":"gateway-v1","index":7,"data":"later-only"}
            ])
        );
        assert!(!request.to_string().contains("_memivy_openai_reasoning"));
        Response::stream(stop("Done"))
    });
    let mut messages = vec![Message::user("Synthetic")];
    for with_reasoning in [false, true] {
        let mut call = rig_core::message::ToolCall::from_wire(
            "reused-call",
            rig_core::message::ToolFunction::new("read_memory".into(), json!({"id":"synthetic"})),
        );
        if with_reasoning {
            call.additional_params = Some(
                json!({"_memivy_openai_reasoning":{"reasoning_details":[{"type":"reasoning.encrypted","id":"later-thought","format":"gateway-v1","index":7,"data":"later-only"}]}}),
            );
        }
        let result = memivy_core::model::tool_result(&call, &json!({"value":"synthetic"}));
        messages.push(Message::Assistant {
            id: None,
            content: vec![memivy_core::model::AssistantContent::ToolCall(call)],
        });
        messages.push(result);
    }
    assert_eq!(
        text(
            &stream_turn(&config, &messages, &offered(), |_| Ok(()))
                .await
                .unwrap()
        ),
        "Done"
    );
    server.join().unwrap();
}

#[tokio::test]
async fn repeated_reasoning_indices_preserve_summary_encrypted_summary_order() {
    let expected = json!([
        {"type":"reasoning.summary","index":0,"id":"shared-summary-id","format":"openai-responses-v1","summary":"First summary."},
        {"type":"reasoning.encrypted","index":0,"id":"encrypted-1","format":"openai-responses-v1","data":"opaque-1"},
        {"type":"reasoning.summary","index":0,"id":"shared-summary-id","format":"openai-responses-v1","summary":"Second summary."},
        {"type":"reasoning.encrypted","index":0,"id":"encrypted-2","format":"openai-responses-v1","data":"opaque-2"},
        {"type":"reasoning.encrypted","index":0,"id":"encrypted-3","format":"openai-responses-v1","data":"opaque-3"}
    ]);
    let (config, _, server) = server(2, move |index, request| {
        if index == 0 {
            let details = [
                json!({"type":"reasoning.summary","index":0,"id":"shared-summary-id","summary":"First "}),
                json!({"type":"reasoning.summary","index":0,"format":"openai-responses-v1","summary":"summary."}),
                expected[1].clone(),
                json!({"type":"reasoning.summary","index":0,"id":"shared-summary-id","summary":"Second "}),
                json!({"type":"reasoning.summary","index":0,"format":"openai-responses-v1","summary":"summary."}),
                expected[3].clone(),
                expected[4].clone(),
            ];
            let body: String = details
                .into_iter()
                .map(|part| delta(json!({"reasoning_details":[part]}), Value::Null))
                .collect();
            Response::stream(body + &call("wire-call", "read_memory", "{\"id\":\"synthetic\"}"))
        } else {
            assert_eq!(request["messages"][1]["reasoning_details"], expected);
            assert!(!request.to_string().contains("_memivy_openai_reasoning"));
            Response::stream(stop("Done"))
        }
    });
    let mut messages = vec![Message::user("Synthetic")];
    let first = stream_turn(&config, &messages, &offered(), |_| Ok(()))
        .await
        .unwrap();
    let result = memivy_core::model::tool_result(
        calls(&first).next().unwrap(),
        &json!({"value":"synthetic"}),
    );
    let saved = serde_json::to_string(&assistant(first)).unwrap();
    messages.push(serde_json::from_str(&saved).unwrap());
    messages.push(result);
    assert_eq!(
        text(
            &stream_turn(&config, &messages, &offered(), |_| Ok(()))
                .await
                .unwrap()
        ),
        "Done"
    );
    server.join().unwrap();
}
