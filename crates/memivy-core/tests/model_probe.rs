use memivy_core::model::{ModelConfig, ProbeError, probe};
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};
fn endpoint(status: u16, body: String, delay: u64) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let thread = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut socket = loop {
            match listener.accept() {
                Ok((socket, _)) => break socket,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if std::time::Instant::now() >= deadline {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) => panic!("{e}"),
            }
        };
        socket.set_nonblocking(false).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut header = vec![];
        while !header.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            if let Err(error) = socket.read_exact(&mut byte) {
                assert!(delay > 0, "request read failed: {error}");
                return;
            }
            header.push(byte[0]);
        }
        let header = String::from_utf8(header).unwrap();
        assert!(header.starts_with("POST /v1/chat/completions "));
        let length = header
            .lines()
            .find_map(|line| {
                line.to_lowercase()
                    .strip_prefix("content-length:")
                    .map(|n| n.trim().parse::<usize>().unwrap())
            })
            .unwrap();
        let mut body_bytes = vec![0; length];
        socket.read_exact(&mut body_bytes).unwrap();
        std::thread::sleep(Duration::from_millis(delay));
        let _ = write!(
            socket,
            "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
    });
    (url, thread)
}
fn config(base_url: String) -> ModelConfig {
    ModelConfig {
        provider: Default::default(),
        base_url,
        model: "test-only".into(),
        api_key: Some("fixture-secret".into()),
        max_output_tokens: None,
        output_token_parameter: Default::default(),
        disable_reasoning: false,
    }
}
fn response(content: &str, finish: &str) -> String {
    serde_json::json!({"id":"fixture","model":"test-only","choices":[{"message":{"role":"assistant","content":content},"finish_reason":finish}]})
        .to_string()
}

#[tokio::test]
async fn validates_exact_schema_and_categorizes_failures_without_leaking() {
    let cases = vec![
        (
            200,
            response(r#"{"ok":true,"echo":"先留住原话"}"#, "stop"),
            None,
        ),
        (
            401,
            "fixture-secret diagnostic".into(),
            Some(ProbeError::Status(401)),
        ),
        (429, "rate limit".into(), Some(ProbeError::Status(429))),
        (
            200,
            response(r#"{"ok":true,"echo":"先留住原话","extra":1}"#, "stop"),
            Some(ProbeError::InvalidResponse),
        ),
        (
            200,
            response(r#"{"ok":true,"echo":"先留住原话"}"#, "length"),
            Some(ProbeError::Truncated),
        ),
        (
            200,
            "invalid JSON".into(),
            Some(ProbeError::InvalidResponse),
        ),
        (200, "x".repeat(65_537), Some(ProbeError::TooLarge)),
    ];
    for (status, body, error) in cases {
        let (url, thread) = endpoint(status, body, 0);
        let result = probe(config(url), Duration::from_secs(10)).await;
        thread.join().unwrap();
        if let Some(error) = error {
            let actual = result.unwrap_err();
            assert_eq!(actual, error);
            assert!(!actual.to_string().contains("fixture-secret"));
        } else {
            assert!(result.unwrap().valid);
        }
    }
}
#[tokio::test]
async fn timeout_does_not_prevent_capture_or_search() {
    let dir = tempfile::tempdir().unwrap();
    use memivy_core::memory::{CaptureRequest, MemoryStore, Origin, SearchRequest};
    let store = MemoryStore::open(dir.path()).unwrap();
    store
        .capture(&CaptureRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            text: "模型失败也保留原话".into(),
            origin: Origin::User {
                app: "test".into(),
                project: None,
                uri: None,
            },
        })
        .unwrap();
    let (url, thread) = endpoint(200, "{}".into(), 200);
    assert_eq!(
        probe(config(url), Duration::from_millis(40))
            .await
            .unwrap_err(),
        ProbeError::Network
    );
    thread.join().unwrap();
    assert_eq!(
        store
            .search(&SearchRequest::text("保留原话", 10))
            .unwrap()
            .items
            .len(),
        1
    );
}
#[tokio::test]
async fn rejects_insecure_remote_or_credentials_in_url() {
    for url in [
        "http://example.com/v1",
        "https://secret@example.com/v1",
        "https://example.com/v1?api_key=secret",
    ] {
        assert_eq!(
            probe(config(url.into()), Duration::from_secs(1))
                .await
                .unwrap_err(),
            ProbeError::Endpoint
        );
    }
}

#[tokio::test]
async fn full_text_accepts_long_output_and_rejects_truncation() {
    use memivy_core::model::{OutputPolicy, complete_with_policy};
    use serde_json::json;
    let long_body = "保留事实、日期与不确定性。".repeat(2500);
    for (finish, valid) in [("stop", true), ("length", false)] {
        let content = json!({"body":long_body}).to_string();
        let (url, server) = endpoint(200, response(&content, finish), 0);
        let result = complete_with_policy(
            &config(url),
            vec![memivy_core::model::Message::user(
                "Clean up this synthetic memory",
            )],
            "cleanup",
            json!({}),
            OutputPolicy::FullText,
        )
        .await;
        server.join().unwrap();
        if valid {
            assert_eq!(result.unwrap()["body"], long_body);
        } else {
            assert_eq!(result.unwrap_err(), ProbeError::Truncated);
        }
    }
}

#[tokio::test]
async fn output_budget_is_optional_and_uses_only_the_selected_parameter() {
    use memivy_core::model::{OutputTokenParameter, complete};
    use serde_json::{Value, json};
    for (limit, parameter, expected) in [
        (None, OutputTokenParameter::MaxTokens, None),
        (
            Some(16000),
            OutputTokenParameter::MaxTokens,
            Some("max_tokens"),
        ),
        (
            Some(32000),
            OutputTokenParameter::MaxCompletionTokens,
            Some("max_completion_tokens"),
        ),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut c = config(format!("http://{}/v1", listener.local_addr().unwrap()));
        c.max_output_tokens = limit;
        c.output_token_parameter = parameter;
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut part = [0; 4096];
            let end = loop {
                let n = socket.read(&mut part).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&part[..n]);
                if let Some(i) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                    break i + 4;
                }
            };
            let length: usize = String::from_utf8_lossy(&bytes[..end])
                .lines()
                .find_map(|s| {
                    s.to_lowercase()
                        .strip_prefix("content-length:")
                        .map(|n| n.trim().parse().unwrap())
                })
                .unwrap();
            while bytes.len() < end + length {
                let n = socket.read(&mut part).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&part[..n]);
            }
            let request: Value = serde_json::from_slice(&bytes[end..end + length]).unwrap();
            for name in ["max_tokens", "max_completion_tokens"] {
                if expected == Some(name) {
                    assert_eq!(request[name], limit.unwrap());
                } else {
                    assert!(request.get(name).is_none());
                }
            }
            let body = response("{}", "stop");
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        complete(
            &c,
            vec![memivy_core::model::Message::user("Synthetic request")],
            "test",
            json!({}),
        )
        .await
        .unwrap();
        server.join().unwrap();
    }
}
