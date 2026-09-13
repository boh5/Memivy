use memivy_core::model::{
    ModelConfig, ProbeError,
    tools::{self, StreamDelta, StreamTurn, function, stream_turn},
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
fn offered() -> Vec<Value> {
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
        &[json!({"role":"user","content":"synthetic"})],
        &offered(),
        |event| {
            chunks.push(event);
            Ok(())
        },
    )
    .await
    .unwrap();
    handle.join().unwrap();
    assert_eq!(
        chunks,
        vec![
            StreamDelta::Text("先记".into()),
            StreamDelta::Text("下来。".into())
        ]
    );
    assert_eq!(result.text, "先记下来。");
    assert!(result.calls.is_empty());
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
    let result = stream_turn(&config, &[], &offered(), |delta| {
        events.push(delta);
        Ok(())
    })
    .await
    .unwrap();
    handle.join().unwrap();
    assert_eq!(result.calls.len(), 1);
    assert_eq!(result.calls[0].arguments, json!({"id":"中文"}));
    assert_eq!(result.calls[0].id, "call-1");
    assert!(result.text.is_empty());
    assert_eq!(result.message["reasoning_content"], "internal context");
    assert!(
        events
            .iter()
            .all(|e| matches!(e, StreamDelta::ToolArguments { .. }))
    );
    let restored: StreamTurn =
        serde_json::from_str(&serde_json::to_string(&result).unwrap()).unwrap();
    assert_eq!(restored.message, result.message);
    assert_eq!(
        restored.message["tool_calls"][0]["function"]["arguments"],
        "{\"id\":\"中文\"}"
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
        (duplicate, ProbeError::InvalidResponse),
        ("data: [DONE]\r\n\r\n".into(), ProbeError::InvalidResponse),
    ];
    for (body, expected) in cases {
        let (config, _, handle) = server(1, move |_, _| Response::stream(body.clone()));
        assert_eq!(
            stream_turn(&config, &[], &offered(), |_| Ok(()))
                .await
                .unwrap_err(),
            expected
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
    let result = stream_turn(&config, &[], &[], |_| {
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
        let error = stream_turn(&config, &[], &[], |_| Ok(()))
            .await
            .unwrap_err();
        assert_eq!(error, expected);
        assert!(!error.to_string().contains("fixture-secret"));
        handle.join().unwrap();
    }
}

#[tokio::test]
async fn capability_probe_exercises_natural_multiturn_streaming_without_mutating_active_cache() {
    let (config, requests, handle) = server(5, |index, request| {
        match index {
        0 => Response {status:200,content_type:"application/json",body:json!({"choices":[{"finish_reason":"stop","message":{"content":"{\"ok\":true,\"echo\":\"先留住原话\"}"}}]}).to_string(),split:false},
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
    let dir = tempfile::tempdir().unwrap();
    let mut active = config.clone();
    active.model = "active".into();
    tools::save_capabilities(
        dir.path(),
        &active,
        &tools::Capabilities {
            structured_json: true,
            streaming_text: true,
            single_tool: true,
            multi_turn: true,
        },
    )
    .unwrap();
    let before = std::fs::read(dir.path().join("model-capabilities.json")).unwrap();
    let tested = tools::probe(&config).await.unwrap();
    handle.join().unwrap();
    assert!(tested.supports_agent() && tested.structured_json && tested.single_tool);
    assert_eq!(
        std::fs::read(dir.path().join("model-capabilities.json")).unwrap(),
        before
    );
    assert!(tools::cached(dir.path(), &config).is_none());
    tools::save_capabilities(dir.path(), &config, &tested).unwrap();
    assert!(tools::cached(dir.path(), &config).unwrap().supports_agent());
    let requests = requests.lock().unwrap();
    assert!(requests[1..].iter().all(|r| r["stream"] == true));
    assert!(requests[2..].iter().all(|r| r["tool_choice"] == "auto"));
}
