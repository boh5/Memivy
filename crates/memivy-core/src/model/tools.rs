//! Bounded protocol loop shared by the two approved memory workflows.
//! Tool effects remain in the caller; terminal calls are returned, never applied here.
use super::*;
use serde_json::Value;
use std::{collections::HashSet, future::Future};

pub fn function(name: &str, description: &str, properties: Value) -> Value {
    let required: Vec<_> = properties.as_object().unwrap().keys().cloned().collect();
    json!({"type":"function","function":{"name":name,"description":description,"strict":true,
        "parameters":{"type":"object","properties":properties,"required":required,"additionalProperties":false}}})
}

pub struct LoopSpec {
    pub messages: Vec<Value>,
    pub tools: Vec<Value>,
    pub terminals: Vec<String>,
    pub evidence_rounds: usize,
}

pub async fn run<C: Clone, H, Fut>(
    config: &ModelConfig,
    spec: LoopSpec,
    context: C,
    handle: H,
) -> Result<(C, FunctionCall), ProbeError>
where
    H: FnMut(C, FunctionCall) -> Fut,
    Fut: Future<Output = Result<(C, Value, bool), ProbeError>>,
{
    run_bounded(
        config,
        spec,
        context,
        handle,
        Duration::from_secs(180),
        Duration::from_secs(20),
        384 * 1024,
    )
    .await
}

async fn run_bounded<C: Clone, H, Fut>(
    config: &ModelConfig,
    spec: LoopSpec,
    mut context: C,
    mut handle: H,
    total: Duration,
    reserve: Duration,
    mut bytes_left: usize,
) -> Result<(C, FunctionCall), ProbeError>
where
    H: FnMut(C, FunctionCall) -> Fut,
    Fut: Future<Output = Result<(C, Value, bool), ProbeError>>,
{
    if !matches!(spec.evidence_rounds, 2 | 4) {
        return Err(ProbeError::InvalidResponse);
    }
    let deadline = Instant::now() + total;
    let mut messages = spec.messages;
    let mut seen = HashSet::new();
    let mut final_only = false;
    let terminal_tools: Vec<_> = spec
        .tools
        .iter()
        .filter(|t| spec.terminals.iter().any(|n| t["function"]["name"] == *n))
        .cloned()
        .collect();
    let terminal_options =
        json!({"tools":terminal_tools,"tool_choice":"required","parallel_tool_calls":false});
    let full_options =
        json!({"tools":spec.tools,"tool_choice":"required","parallel_tool_calls":false});
    let request_size = |messages: &Vec<Value>, options: &Value| -> Result<usize, ProbeError> {
        Ok(
            serde_json::to_vec(&request_body(config, json!(messages), options.clone()))
                .map_err(|_| ProbeError::InvalidResponse)?
                .len(),
        )
    };
    for round in 0..=spec.evidence_rounds {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(ProbeError::Network);
        }
        final_only |= round == spec.evidence_rounds || remaining <= reserve;
        let full_size = request_size(&messages, &full_options)?;
        let final_size = request_size(&messages, &terminal_options)?;
        // Keep room for a valid terminal request plus protocol result markers.
        final_only |= full_size > 96 * 1024
            || full_size.saturating_add(final_size).saturating_add(4096) > bytes_left;
        let options = if final_only {
            terminal_options.clone()
        } else {
            full_options.clone()
        };
        let size = request_size(&messages, &options)?;
        if size > 96 * 1024 || size > bytes_left {
            return Err(ProbeError::TooLarge);
        }
        bytes_left -= size;
        let timeout = if final_only {
            remaining
        } else {
            remaining.saturating_sub(reserve)
        }
        .min(Duration::from_secs(90));
        let response = tokio::time::timeout(
            timeout,
            request(
                config,
                json!(messages),
                options.clone(),
                OutputPolicy::Structured,
            ),
        )
        .await;
        let choice = match response {
            Ok(result) => result?,
            Err(_) if !final_only => {
                final_only = true;
                continue;
            }
            Err(_) => return Err(ProbeError::Network),
        };
        let (message, calls) = parse_calls(choice)?;
        for (id, call) in &calls {
            if !seen.insert(id.clone())
                || !options["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|t| t["function"]["name"] == call.name)
            {
                return Err(ProbeError::InvalidResponse);
            }
        }
        if calls.iter().any(|(_, c)| spec.terminals.contains(&c.name)) {
            if calls.len() != 1 {
                return Err(ProbeError::InvalidResponse);
            }
            return Ok((context, calls.into_iter().next().unwrap().1));
        }
        if final_only {
            return Err(ProbeError::InvalidResponse);
        }
        // Preserve all provider protocol fields; never publish them as UI text.
        messages.push(message);
        let chain_minimum =
            request_size(&messages, &terminal_options)?.saturating_add(calls.len() * 256);
        if chain_minimum > 96 * 1024 || chain_minimum > bytes_left {
            // No call has executed: finish from the previous valid conversation
            // rather than retaining an assistant turn whose results cannot fit.
            messages.pop();
            final_only = true;
            continue;
        }
        let mut progress = false;
        let count = calls.len();
        for (index, (id, call)) in calls.into_iter().enumerate() {
            let previous = context.clone();
            let remaining = deadline.saturating_duration_since(Instant::now());
            let outcome = if remaining <= reserve || final_only {
                None
            } else {
                tokio::time::timeout(remaining - reserve, handle(context, call))
                    .await
                    .ok()
            };
            let (next, result, changed) = match outcome {
                Some(result) => result?,
                None => {
                    final_only = true;
                    (
                        previous.clone(),
                        json!({"budget_reached":true,"instruction":"Finish using evidence already supplied."}),
                        false,
                    )
                }
            };
            let reply = json!({"role":"tool","tool_call_id":id,"content":result.to_string()});
            messages.push(reply);
            // Do not register evidence the model never received. Roll back the
            // read-only context if its result would crowd out final generation.
            let required =
                request_size(&messages, &terminal_options)?.saturating_add((count - index) * 256);
            if required > 96 * 1024 || required > bytes_left {
                messages.pop();
                messages.push(
                    json!({"role":"tool","tool_call_id":id,"content":"{\"budget_reached\":true}"}),
                );
                context = previous;
                final_only = true;
            } else {
                context = next;
                progress |= changed;
            }
        }
        final_only |= !progress;
    }
    Err(ProbeError::InvalidResponse)
}

fn parse_calls(choice: Value) -> Result<(Value, Vec<(String, FunctionCall)>), ProbeError> {
    if choice["finish_reason"] != "tool_calls" || choice["message"]["role"] != "assistant" {
        return Err(ProbeError::InvalidResponse);
    }
    let message = choice["message"].clone();
    let raw = message["tool_calls"]
        .as_array()
        .ok_or(ProbeError::InvalidResponse)?;
    if raw.is_empty() || raw.len() > 8 {
        return Err(ProbeError::InvalidResponse);
    }
    let calls = raw
        .iter()
        .map(|c| {
            let id = c["id"].as_str().ok_or(ProbeError::InvalidResponse)?;
            let f = &c["function"];
            let name = f["name"].as_str().ok_or(ProbeError::InvalidResponse)?;
            if c["type"] != "function" || id.is_empty() || id.len() > 128 || name.len() > 80 {
                return Err(ProbeError::InvalidResponse);
            }
            Ok((
                id.to_owned(),
                FunctionCall {
                    name: name.into(),
                    arguments: serde_json::from_str(
                        f["arguments"].as_str().ok_or(ProbeError::InvalidResponse)?,
                    )
                    .map_err(|_| ProbeError::InvalidResponse)?,
                },
            ))
        })
        .collect::<Result<_, _>>()?;
    Ok((message, calls))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capabilities {
    pub structured_json: bool,
    pub single_tool: bool,
    pub multi_turn: bool,
}
#[derive(Serialize, Deserialize)]
struct Cached {
    fingerprint: String,
    capabilities: Capabilities,
}
fn fingerprint(config: &ModelConfig) -> Result<String, ProbeError> {
    use sha2::{Digest, Sha256};
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(config).map_err(|_| ProbeError::Configuration)?)
    ))
}
pub fn cached(root: &Path, config: &ModelConfig) -> Option<Capabilities> {
    let data: Cached =
        serde_json::from_slice(&fs::read(root.join("model-capabilities.json")).ok()?).ok()?;
    (data.fingerprint == fingerprint(config).ok()?).then_some(data.capabilities)
}
fn supported<T>(result: Result<T, ProbeError>) -> Result<bool, ProbeError> {
    match result {
        Ok(_) => Ok(true),
        Err(ProbeError::InvalidResponse | ProbeError::Status(400 | 422)) => Ok(false),
        Err(e) => Err(e),
    }
}
pub async fn probe_and_save(root: &Path, config: &ModelConfig) -> Result<Capabilities, ProbeError> {
    let structured_json = supported(super::probe(config.clone(), Duration::from_secs(45)).await)?;
    let lookup = function(
        "probe_lookup",
        "Read a fixed synthetic test value",
        json!({"key":{"type":"string","enum":["synthetic"]}}),
    );
    let finish = function(
        "probe_finish",
        "Return the test result",
        json!({"value":{"type":"string"}}),
    );
    let messages = vec![
        json!({"role":"user","content":"This is a synthetic protocol test. Call probe_lookup with key synthetic. After its result, call probe_finish with the exact returned value."}),
    ];
    let first = tokio::time::timeout(
        Duration::from_secs(45),
        request(
            config,
            json!(messages),
            json!({"tools":[lookup],"tool_choice":"required","parallel_tool_calls":false}),
            OutputPolicy::Structured,
        ),
    )
    .await
    .map_err(|_| ProbeError::Network)?;
    let first = first.and_then(parse_calls).and_then(|(message, calls)| {
        if calls.len() != 1
            || calls[0].1.name != "probe_lookup"
            || calls[0].1.arguments != json!({"key":"synthetic"})
        {
            return Err(ProbeError::InvalidResponse);
        }
        Ok((message, calls.into_iter().next().unwrap().0))
    });
    let (single_tool, multi_turn) = match first {
        Ok((message, id)) => {
            let mut messages = messages;
            messages.push(message);
            messages.push(json!({"role":"tool","tool_call_id":id,"content":"MEMIVY-PROTOCOL-47"}));
            let next = tokio::time::timeout(
                Duration::from_secs(45),
                request(
                    config,
                    json!(messages),
                    json!({"tools":[finish],"tool_choice":"required","parallel_tool_calls":false}),
                    OutputPolicy::Structured,
                ),
            )
            .await
            .map_err(|_| ProbeError::Network)?;
            let checked = next.and_then(parse_calls).and_then(|(_, calls)| {
                if calls.len() == 1
                    && calls[0].1.name == "probe_finish"
                    && calls[0].1.arguments == json!({"value":"MEMIVY-PROTOCOL-47"})
                {
                    Ok(())
                } else {
                    Err(ProbeError::InvalidResponse)
                }
            });
            (true, supported(checked)?)
        }
        Err(e) => (supported::<()>(Err(e))?, false),
    };
    let capabilities = Capabilities {
        structured_json,
        single_tool,
        multi_turn,
    };
    save_private_json(
        &root.join("model-capabilities.json"),
        &Cached {
            fingerprint: fingerprint(config)?,
            capabilities: capabilities.clone(),
        },
    )?;
    Ok(capabilities)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
    };
    fn fixture(
        first_delay: Duration,
    ) -> (
        ModelConfig,
        Arc<Mutex<Vec<Value>>>,
        std::thread::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let config = ModelConfig {
            base_url: format!("http://{}/v1", listener.local_addr().unwrap()),
            model: "budget-test".into(),
            api_key: None,
            max_output_tokens: None,
            output_token_parameter: Default::default(),
            disable_reasoning: false,
        };
        let log = Arc::new(Mutex::new(vec![]));
        let requests = log.clone();
        let server = std::thread::spawn(move || {
            for n in 0..2 {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut headers = vec![];
                while !headers.ends_with(b"\r\n\r\n") {
                    let mut b = [0];
                    socket.read_exact(&mut b).unwrap();
                    headers.push(b[0]);
                }
                let len = String::from_utf8(headers)
                    .unwrap()
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                let mut data = vec![0; len];
                socket.read_exact(&mut data).unwrap();
                requests
                    .lock()
                    .unwrap()
                    .push(serde_json::from_slice(&data).unwrap());
                if n == 0 {
                    std::thread::sleep(first_delay);
                }
                let name = if n == 0 { "read" } else { "finish" };
                let body=json!({"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","tool_calls":[{"id":format!("c{n}"),"type":"function","function":{"name":name,"arguments":"{}"}}]}}]}).to_string();
                let _ = write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        (config, log, server)
    }
    fn spec() -> LoopSpec {
        LoopSpec {
            messages: vec![json!({"role":"user","content":"synthetic"})],
            tools: vec![
                function("read", "read", json!({})),
                function("finish", "finish", json!({})),
            ],
            terminals: vec!["finish".into()],
            evidence_rounds: 4,
        }
    }
    #[tokio::test]
    async fn final_time_is_reserved_after_slow_evidence_request() {
        let (config, requests, server) = fixture(Duration::from_millis(140));
        let (state, call) = run_bounded(
            &config,
            spec(),
            0,
            |state, _| async move { Ok((state + 1, json!({}), true)) },
            Duration::from_millis(300),
            Duration::from_millis(200),
            384 * 1024,
        )
        .await
        .unwrap();
        server.join().unwrap();
        assert_eq!(state, 0);
        assert_eq!(call.name, "finish");
        assert_eq!(
            requests.lock().unwrap()[1]["tools"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
    #[tokio::test]
    async fn oversized_tool_result_keeps_final_request_and_rolls_back_unseen_evidence() {
        let (config, requests, server) = fixture(Duration::ZERO);
        let (state, call) = run_bounded(
            &config,
            spec(),
            0,
            |state, _| async move { Ok((state + 1, json!({"text":"x".repeat(30_000)}), true)) },
            Duration::from_secs(2),
            Duration::from_millis(500),
            10_000,
        )
        .await
        .unwrap();
        server.join().unwrap();
        assert_eq!(state, 0);
        assert_eq!(call.name, "finish");
        let log = requests.lock().unwrap();
        assert!(log.iter().map(|r| r.to_string().len()).sum::<usize>() <= 10_000);
        assert!(log[1].to_string().contains("budget_reached"));
        assert!(!log[1].to_string().contains(&"x".repeat(100)));
    }
}
