use memivy_core::model::{ModelConfig, ProbeError, probe};
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};
fn endpoint(status: u16, body: String, delay: u64) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let thread = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = vec![0; 8192];
        let size = socket.read(&mut request).unwrap();
        let text = String::from_utf8_lossy(&request[..size]);
        assert!(text.starts_with("POST /v1/chat/completions "));
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
        base_url,
        model: "test-only".into(),
        api_key: Some("fixture-secret".into()),
        disable_reasoning: false,
    }
}
fn response(content: &str, finish: &str) -> String {
    serde_json::json!({"choices":[{"message":{"content":content},"finish_reason":finish}]})
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
            Some(ProbeError::InvalidResponse),
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
        let result = probe(config(url), Duration::from_secs(2)).await;
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
    let store =
        memivy_core::Store::open(memivy_core::DataPaths::new(dir.path().into()).unwrap()).unwrap();
    store
        .capture(memivy_core::CaptureInput {
            request_id: uuid::Uuid::new_v4().to_string(),
            text: "模型失败也保留原话".into(),
            source_app: "test".into(),
            project: None,
            session_uri: None,
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
    assert_eq!(store.search("保留原话", 10).unwrap().items.len(), 1);
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
async fn tool_responses_require_one_complete_function_call() {
    use memivy_core::model::call_function;
    use serde_json::json;
    let call = json!({"type":"function","function":{"name":"create_memory","arguments":"{\"title\":\"模型标题\"}"}});
    for (calls, finish, refusal, valid) in [
        (json!([call]), "tool_calls", json!(null), true),
        (json!([]), "tool_calls", json!(null), false),
        (json!([call, call]), "tool_calls", json!(null), false),
        (json!([call]), "length", json!(null), false),
        (json!([call]), "stop", json!(null), false),
        (json!([call]), "tool_calls", json!("refused"), false),
    ] {
        let response=json!({"choices":[{"finish_reason":finish,"message":{"tool_calls":calls,"refusal":refusal}}]}).to_string();
        let (url, server) = endpoint(200, response, 0);
        let result = call_function(&config(url), json!([]), json!([])).await;
        server.join().unwrap();
        assert_eq!(result.is_ok(), valid);
        if let Ok(result) = result {
            assert_eq!(result.arguments["title"], "模型标题");
        }
    }
}
