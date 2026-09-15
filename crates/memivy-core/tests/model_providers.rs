use memivy_core::{
    memory::*,
    model::{self, Message, ModelConfig, ProbeError, Provider, tools},
};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const PROVIDERS: [Provider; 4] = [
    Provider::OpenaiCompatible,
    Provider::OpenaiResponses,
    Provider::Anthropic,
    Provider::Gemini,
];
fn sse(events: Vec<Value>) -> String {
    events
        .into_iter()
        .map(|v| format!("data: {v}\n\n"))
        .collect()
}
fn reply(
    provider: Provider,
    streamed: bool,
    tool: Option<(&str, Value)>,
    text: &str,
    truncated: bool,
) -> String {
    let name = tool.as_ref().map(|t| t.0).unwrap_or("");
    let args = tool.as_ref().map(|t| t.1.clone()).unwrap_or(json!({}));
    match provider {
        Provider::OpenaiCompatible => {
            let finish = if truncated {
                "length"
            } else if tool.is_some() {
                "tool_calls"
            } else {
                "stop"
            };
            let message = if tool.is_some() {
                json!({"role":"assistant","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":name,"arguments":args.to_string()}}]})
            } else {
                json!({"role":"assistant","content":text})
            };
            if !streamed {
                return json!({"id":"response_1","object":"chat.completion","created":1,"model":"user-defined-model","choices":[{"index":0,"message":message,"finish_reason":finish}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}).to_string();
            }
            let delta = if tool.is_some() {
                json!({"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":name,"arguments":args.to_string()}}]})
            } else {
                json!({"content":text})
            };
            sse(vec![
                json!({"choices":[{"index":0,"delta":delta,"finish_reason":null}]}),
                json!({"choices":[{"index":0,"delta":{},"finish_reason":finish}]}),
            ]) + "data: [DONE]\n\n"
        }
        Provider::Anthropic => {
            let reason = if truncated {
                "max_tokens"
            } else if tool.is_some() {
                "tool_use"
            } else {
                "end_turn"
            };
            let block = if tool.is_some() {
                json!({"type":"tool_use","id":"call_1","name":name,"input":args})
            } else {
                json!({"type":"text","text":text})
            };
            let response = json!({"id":"msg_1","type":"message","role":"assistant","model":"user-defined-model","content":[block],"stop_reason":reason,"stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":1}});
            if !streamed {
                return response.to_string();
            }
            let mut start = response.clone();
            start["content"] = json!([]);
            start["stop_reason"] = Value::Null;
            let mut empty = block;
            let delta = if tool.is_some() {
                empty["input"] = json!({});
                json!({"type":"input_json_delta","partial_json":args.to_string()})
            } else {
                empty["text"] = json!("");
                json!({"type":"text_delta","text":text})
            };
            sse(vec![
                json!({"type":"message_start","message":start}),
                json!({"type":"content_block_start","index":0,"content_block":empty}),
                json!({"type":"content_block_delta","index":0,"delta":delta}),
                json!({"type":"content_block_stop","index":0}),
                json!({"type":"message_delta","delta":{"stop_reason":reason,"stop_sequence":null},"usage":{"output_tokens":1}}),
                json!({"type":"message_stop"}),
            ])
        }
        Provider::Gemini => {
            let part = if tool.is_some() {
                json!({"functionCall":{"name":name,"args":args},"thoughtSignature":"fixture-signature"})
            } else {
                json!({"text":text})
            };
            let response = json!({"candidates":[{"content":{"role":"model","parts":[part]},"finishReason":if truncated {"MAX_TOKENS"} else {"STOP"},"index":0}],"usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":1,"totalTokenCount":2},"modelVersion":"user-defined-model"});
            if streamed {
                sse(vec![response])
            } else {
                response.to_string()
            }
        }
        Provider::OpenaiResponses => {
            let output = if tool.is_some() {
                json!({"type":"function_call","id":"fc_1","call_id":"call_1","name":name,"arguments":args.to_string(),"status":"completed"})
            } else {
                json!({"type":"message","id":"msg_1","role":"assistant","status":"completed","content":[{"type":"output_text","text":text,"annotations":[]}]})
            };
            let mut response = json!({"id":"resp_1","object":"response","created_at":1,"status":"completed","model":"user-defined-model","output":[output],"tools":[],"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}});
            if truncated {
                response["status"] = json!("incomplete");
                response["incomplete_details"] = json!({"reason":"max_output_tokens"});
            }
            if !streamed {
                return response.to_string();
            }
            let mut empty = output.clone();
            empty["status"] = json!("in_progress");
            if tool.is_some() {
                empty["arguments"] = json!("");
            } else {
                empty["content"] = json!([]);
            }
            let mut start = response.clone();
            start["status"] = json!("in_progress");
            start["output"] = json!([]);
            let mut events = vec![
                json!({"type":"response.created","response":start}),
                json!({"type":"response.output_item.added","output_index":0,"item":empty}),
            ];
            if tool.is_some() {
                events.extend([json!({"type":"response.function_call_arguments.delta","item_id":"fc_1","output_index":0,"delta":args.to_string()}),json!({"type":"response.function_call_arguments.done","item_id":"fc_1","output_index":0,"arguments":args.to_string()})]);
            } else {
                events.extend([json!({"type":"response.content_part.added","item_id":"msg_1","output_index":0,"content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}),json!({"type":"response.output_text.delta","item_id":"msg_1","output_index":0,"content_index":0,"delta":text}),json!({"type":"response.output_text.done","item_id":"msg_1","output_index":0,"content_index":0,"text":text}),json!({"type":"response.content_part.done","item_id":"msg_1","output_index":0,"content_index":0,"part":output["content"][0]})]);
            }
            events.extend([json!({"type":"response.output_item.done","output_index":0,"item":output}),json!({"type":if truncated {"response.incomplete"} else {"response.completed"},"response":response})]);
            for (i, event) in events.iter_mut().enumerate() {
                event["sequence_number"] = json!(i);
            }
            sse(events)
        }
    }
}

type Requests = Arc<Mutex<Vec<(String, Value)>>>;
fn fixture(
    provider: Provider,
    count: usize,
    streamed: bool,
    respond: impl Fn(usize) -> String + Send + 'static,
) -> (ModelConfig, Requests, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base = format!(
        "http://{}{}",
        listener.local_addr().unwrap(),
        if matches!(
            provider,
            Provider::OpenaiCompatible | Provider::OpenaiResponses
        ) {
            "/v1"
        } else {
            ""
        }
    );
    let config = ModelConfig {
        provider,
        base_url: base,
        model: "user-defined-model".into(),
        api_key: Some("fixture-secret".into()),
        disable_reasoning: false,
        max_output_tokens: Some(4096),
        output_token_parameter: Default::default(),
    };
    let requests: Requests = Arc::default();
    let record = requests.clone();
    let thread = std::thread::spawn(move || {
        for i in 0..count {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "missing request {i}");
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut header = vec![];
            while !header.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                socket.read_exact(&mut byte).unwrap();
                header.push(byte[0]);
            }
            let header = String::from_utf8(header).unwrap();
            let length = header
                .lines()
                .find_map(|line| {
                    line.to_lowercase()
                        .strip_prefix("content-length:")
                        .map(|n| n.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            let mut body = vec![0; length];
            socket.read_exact(&mut body).unwrap();
            record
                .lock()
                .unwrap()
                .push((header, serde_json::from_slice(&body).unwrap()));
            let body = respond(i);
            let content_type = if streamed {
                "text/event-stream"
            } else {
                "application/json"
            };
            let _ = write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (config, requests, thread)
}
fn assert_request(provider: Provider, header: &str, body: &Value, streamed: bool) {
    let path = match provider {
        Provider::OpenaiCompatible => "/v1/chat/completions",
        Provider::OpenaiResponses => "/v1/responses",
        Provider::Anthropic => "/v1/messages",
        Provider::Gemini if streamed => "/v1beta/models/user-defined-model:streamGenerateContent",
        Provider::Gemini => "/v1beta/models/user-defined-model:generateContent",
    };
    assert!(
        header.starts_with(&format!("POST {path}")),
        "{provider:?} path"
    );
    if provider != Provider::Gemini {
        assert_eq!(body["model"], "user-defined-model");
    }
    match provider {
        Provider::OpenaiCompatible | Provider::OpenaiResponses => assert!(
            header
                .to_lowercase()
                .contains("authorization: bearer fixture-secret")
        ),
        Provider::Anthropic => assert!(header.to_lowercase().contains("x-api-key: fixture-secret")),
        Provider::Gemini => assert!(
            header
                .lines()
                .next()
                .unwrap()
                .contains("key=fixture-secret")
        ),
    }
}

#[tokio::test]
async fn all_protocols_support_arbitrary_model_ids_and_structured_requests() {
    for provider in PROVIDERS {
        let (config, requests, server) = fixture(provider, 1, false, move |_| {
            reply(provider, false, None, "{\"ok\":true}", false)
        });
        let value = model::complete(&config, vec![Message::system("Return the requested JSON."), Message::user("Synthetic test")], "fixture", json!({"type":"object","properties":{"ok":{"type":"boolean"}},"required":["ok"],"additionalProperties":false})).await.unwrap();
        server.join().unwrap();
        assert_eq!(value, json!({"ok":true}));
        let requests = requests.lock().unwrap();
        assert_request(provider, &requests[0].0, &requests[0].1, false);
        let body = &requests[0].1;
        let schema = match provider {
            Provider::OpenaiCompatible => &body["response_format"]["json_schema"]["schema"],
            Provider::OpenaiResponses => &body["text"]["format"]["schema"],
            Provider::Anthropic => &body["output_config"]["format"]["schema"],
            Provider::Gemini => &body["generationConfig"]["responseJsonSchema"],
        };
        assert!(
            schema.is_object(),
            "{provider:?} structured schema missing: {body}"
        );
    }
}

#[tokio::test]
async fn all_protocols_roundtrip_tool_results_and_provider_signatures() {
    for provider in PROVIDERS {
        let (config, requests, server) = fixture(provider, 2, true, move |i| {
            reply(
                provider,
                true,
                (i == 0).then(|| ("lookup", json!({"key":"synthetic"}))),
                "DONE",
                false,
            )
        });
        let definitions = vec![tools::function(
            "lookup",
            "Read a synthetic value",
            json!({"key":{"type":"string"}}),
        )];
        let mut messages = vec![
            Message::system("Use tools when needed."),
            Message::user("Read synthetic data"),
        ];
        let response = tools::stream_turn(&config, &messages, &definitions, |_| Ok(()))
            .await
            .unwrap();
        let call = model::calls(&response).next().unwrap();
        assert_eq!(call.function.arguments, json!({"key":"synthetic"}));
        if provider == Provider::Gemini {
            assert_eq!(call.signature.as_deref(), Some("fixture-signature"));
        }
        let result = model::tool_result(call, &json!({"value":"saved-value"}));
        let saved = serde_json::to_string(&model::assistant(response)).unwrap();
        messages.push(serde_json::from_str(&saved).unwrap());
        messages.push(result);
        let response = tools::stream_turn(&config, &messages, &definitions, |_| Ok(()))
            .await
            .unwrap();
        assert_eq!(model::text(&response), "DONE");
        server.join().unwrap();
        let requests = requests.lock().unwrap();
        for (header, body) in requests.iter() {
            assert_request(provider, header, body, true);
        }
        let second = &requests[1].1;
        assert!(second.to_string().contains("saved-value"));
        match provider {
            Provider::OpenaiCompatible => {
                assert_eq!(second["messages"][3]["tool_call_id"], "call_1")
            }
            Provider::OpenaiResponses => {
                assert!(
                    second["input"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|v| v["type"] == "function_call_output" && v["call_id"] == "call_1")
                );
            }
            Provider::Anthropic => assert!(
                second["messages"]
                    .to_string()
                    .contains("\"tool_use_id\":\"call_1\"")
            ),
            Provider::Gemini => {
                assert!(second["contents"].to_string().contains("fixture-signature"))
            }
        }
    }
}

#[tokio::test]
async fn all_protocols_preserve_organization_receipts_and_reject_truncated_writes() {
    for provider in PROVIDERS {
        for truncated in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let store = MemoryStore::open(dir.path()).unwrap();
            let raw = store
                .capture(&CaptureRequest {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    text: "Offline operation remains required.".into(),
                    origin: Origin::User {
                        app: "provider fixture".into(),
                        project: None,
                        uri: None,
                    },
                })
                .unwrap();
            let mut task = store.claim_organization().unwrap().unwrap();
            store.prepare_organization(&mut task).unwrap();
            let args = json!({"destination":{"kind":"new"},"title":"Offline requirement","parts":[{"text":"Offline operation remains required.","sources":[{"source_id":raw.capture_id,"quote":"Offline operation remains required."}]}]});
            let (config, requests, server) = fixture(provider, 1, true, move |_| {
                reply(
                    provider,
                    true,
                    Some(("write_memory", args.clone())),
                    "",
                    truncated,
                )
            });

            let result = store.run_organization(&config, &task).await;
            server.join().unwrap();
            assert_eq!(requests.lock().unwrap().len(), 1);
            if truncated {
                assert_eq!(result.unwrap_err(), ProbeError::Truncated);
            } else {
                assert_eq!(
                    result.unwrap().unwrap().memory_id.as_deref(),
                    Some(raw.memory_id.as_str())
                );
            }
            let db = rusqlite::Connection::open(store.database_path()).unwrap();
            let versions: i64 = db
                .query_row(
                    "SELECT count(*) FROM memory_versions WHERE memory_id=?",
                    [&raw.memory_id],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(versions, if truncated { 1 } else { 2 });
        }
    }
}

#[tokio::test]
async fn explicit_refusals_cannot_commit_accompanying_tool_calls() {
    for provider in [Provider::OpenaiCompatible, Provider::OpenaiResponses] {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let raw = store
            .capture(&CaptureRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                text: "Keep the original text.".into(),
                origin: Origin::User {
                    app: "refusal fixture".into(),
                    project: None,
                    uri: None,
                },
            })
            .unwrap();
        let mut task = store.claim_organization().unwrap().unwrap();
        store.prepare_organization(&mut task).unwrap();
        let args = json!({"destination":{"kind":"new"},"title":"Original","parts":[{"text":"Keep the original text.","sources":[{"source_id":raw.capture_id,"quote":"Keep the original text."}]}]});
        let (config, _, server) = fixture(provider, 1, true, move |_| {
            let refusal = match provider {
                Provider::OpenaiCompatible => {
                    json!({"choices":[{"index":0,"delta":{"refusal":"Declined"},"finish_reason":null}]})
                }
                _ => {
                    json!({"type":"response.refusal.delta","sequence_number":0,"output_index":0,"content_index":0,"item_id":"msg_refused","delta":"Declined"})
                }
            };
            sse(vec![refusal])
                + &reply(
                    provider,
                    true,
                    Some(("write_memory", args.clone())),
                    "",
                    false,
                )
        });

        assert_eq!(
            store.run_organization(&config, &task).await.unwrap_err(),
            ProbeError::InvalidResponse
        );
        server.join().unwrap();
        let db = rusqlite::Connection::open(store.database_path()).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM memory_versions WHERE memory_id=?",
                [&raw.memory_id],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    }
}

#[tokio::test]
async fn responses_can_supply_complete_arguments_only_in_the_done_event() {
    let provider = Provider::OpenaiResponses;
    let (config, _, server) = fixture(provider, 1, true, move |_| {
        let body = reply(
            provider,
            true,
            Some(("lookup", json!({"key":"synthetic"}))),
            "",
            false,
        );
        let events = body
            .split("\n\n")
            .filter_map(|line| line.strip_prefix("data: "))
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .filter(|event| {
                !event["type"]
                    .as_str()
                    .unwrap_or_default()
                    .starts_with("response.function_call_arguments.")
            })
            .collect();
        sse(events)
    });
    let response = tools::stream_turn(
        &config,
        &[Message::user("Read synthetic data")],
        &[tools::function(
            "lookup",
            "Read a synthetic value",
            json!({"key":{"type":"string"}}),
        )],
        |_| Ok(()),
    )
    .await
    .unwrap();
    server.join().unwrap();
    assert_eq!(
        model::calls(&response).next().unwrap().function.arguments,
        json!({"key":"synthetic"})
    );
}

#[tokio::test]
async fn responses_terminal_only_refusal_rejects_otherwise_valid_calls() {
    let provider = Provider::OpenaiResponses;
    let (config, _, server) = fixture(provider, 1, true, move |_| {
        let body = reply(
            provider,
            true,
            Some(("lookup", json!({"key":"synthetic"}))),
            "",
            false,
        );
        let events = body
            .split("\n\n")
            .filter_map(|line| line.strip_prefix("data: "))
            .map(|line| {
                let mut event: Value = serde_json::from_str(line).unwrap();
                if event["type"] == "response.completed" {
                    event["response"]["output"].as_array_mut().unwrap().push(json!({
                        "type":"message","id":"msg_refused","role":"assistant","status":"completed",
                        "content":[{"type":"refusal","refusal":"Declined"}]
                    }));
                }
                event
            })
            .collect();
        sse(events)
    });
    let result = tools::stream_turn(
        &config,
        &[Message::user("Read synthetic data")],
        &[tools::function(
            "lookup",
            "Read a synthetic value",
            json!({"key":{"type":"string"}}),
        )],
        |_| Ok(()),
    )
    .await;
    server.join().unwrap();
    assert_eq!(result.unwrap_err(), ProbeError::InvalidResponse);
}

#[tokio::test]
async fn changing_protocol_scrubs_private_chat_reasoning_from_every_provider_request() {
    for provider in [
        Provider::OpenaiResponses,
        Provider::Anthropic,
        Provider::Gemini,
    ] {
        let (config, requests, server) = fixture(provider, 1, true, move |_| {
            reply(provider, true, None, "Done", false)
        });
        let call = model::ToolCall::from_wire("old-chat-call", rig_core::message::ToolFunction::new("lookup".into(), json!({"key":"synthetic"})))
            .with_additional_params(Some(json!({"_memivy_openai_reasoning":{"reasoning_details":[{"type":"reasoning.encrypted","data":"private-chat-only-metadata"}]}})));
        let result = model::tool_result(&call, &json!({"value":"synthetic"}));
        let messages = vec![
            Message::user("Continue the synthetic tool turn"),
            Message::Assistant {
                id: None,
                content: vec![model::AssistantContent::ToolCall(call)],
            },
            result,
        ];
        let output = tools::stream_turn(
            &config,
            &messages,
            &[tools::function(
                "lookup",
                "Read synthetic data",
                json!({"key":{"type":"string"}}),
            )],
            |_| Ok(()),
        )
        .await
        .unwrap();
        assert_eq!(model::text(&output), "Done");
        server.join().unwrap();
        let requests = requests.lock().unwrap();
        let body = requests[0].1.to_string();
        assert!(!body.contains("_memivy_openai_reasoning"), "{provider:?}");
        assert!(!body.contains("private-chat-only-metadata"), "{provider:?}");
    }
}
