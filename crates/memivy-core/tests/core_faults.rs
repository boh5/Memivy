use memivy_core::{
    memory::*,
    model::{ModelConfig, ProbeError},
};
use serde::Deserialize;
use serde_json::json;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
    time::{Duration, Instant},
};
use uuid::Uuid;
const KEY: &str = "synthetic_api_key_never_export_47291";
fn id() -> String {
    Uuid::new_v4().to_string()
}
fn capture(s: &MemoryStore) -> RawCapture {
    let saved = s
        .capture(&CaptureRequest {
            request_id: id(),
            text: " \n故障中的原话 SQLite 中文\n ".into(),
            origin: Origin::User {
                app: "fixture".into(),
                project: None,
                uri: None,
            },
        })
        .unwrap();
    s.capture_by_id(&saved.capture_id).unwrap()
}
fn proposal(task: &OrganizationTask) -> MemoryWriteArgs {
    MemoryWriteArgs {
        destination: Destination::New,
        title: "故障恢复".into(),
        parts: vec![MemoryWritePart {
            text: "故障中的原话 SQLite 中文 回归".into(),
            sources: vec![MemorySourceQuote {
                source_id: task.capture_id.clone(),
                quote: "故障中的原话 SQLite 中文".into(),
            }],
        }],
    }
}
fn counts(s: &MemoryStore) -> Vec<i64> {
    let db = rusqlite::Connection::open(s.database_path()).unwrap();
    [
        "captures",
        "memories",
        "memory_versions",
        "receipts",
        "memory_keywords",
    ]
    .iter()
    .map(|t| {
        db.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))
            .unwrap()
    })
    .collect()
}
#[derive(Deserialize)]
struct Fault {
    id: String,
    mode: String,
    expected: String,
    reason: String,
}
// A real local HTTP endpoint; request bodies and Authorization are never logged.
fn endpoint(mode: &str) -> (ModelConfig, mpsc::Sender<()>, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let config = ModelConfig {
        base_url: format!("http://{}/v1", listener.local_addr().unwrap()),
        model: "synthetic-only".into(),
        api_key: Some(KEY.into()),
        max_output_tokens: None,
        output_token_parameter: Default::default(),
        disable_reasoning: false,
    };
    let mode = mode.to_string();
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let started = Instant::now();
        let mut socket = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(started.elapsed() < Duration::from_secs(10));
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) => panic!("{e}"),
            }
        };
        socket.set_nonblocking(false).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut bytes = vec![];
        let end = loop {
            let mut part = [0; 4096];
            let n = socket.read(&mut part).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&part[..n]);
            if let Some(p) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                break p + 4;
            }
        };
        let size: usize = String::from_utf8_lossy(&bytes[..end])
            .lines()
            .find_map(|l| {
                l.to_lowercase()
                    .strip_prefix("content-length:")
                    .map(|v| v.trim().parse().unwrap())
            })
            .unwrap();
        while bytes.len() < end + size {
            let mut part = [0; 4096];
            let n = socket.read(&mut part).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&part[..n]);
        }
        if mode == "disconnect" {
            return;
        }
        if mode == "timeout" {
            let _ = rx.recv_timeout(Duration::from_secs(100));
            return;
        }
        let request: serde_json::Value = serde_json::from_slice(&bytes[end..end + size]).unwrap();
        let input: serde_json::Value = serde_json::from_str(
            request["messages"].as_array().unwrap().last().unwrap()["content"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        let event = |delta: serde_json::Value, finish: &str| {
            format!(
                "data: {}\n\ndata: [DONE]\n\n",
                json!({"choices":[{"index":0,"delta":delta,"finish_reason":finish}]})
            )
        };
        let (status, body) = match mode.as_str() {
            "unknown_target" => (
                200,
                event(
                    json!({"role":"assistant","tool_calls":[{"index":0,"id":"synthetic-call","type":"function","function":{"name":"write_memory","arguments":json!({"destination":{"kind":"existing","memory_id":id(),"expected_version":id()},"title":"故障恢复","parts":[{"text":"原话","sources":[{"source_id":input["capture_id"],"quote":"故障中的原话"}]}]}).to_string()}}]}),
                    "tool_calls",
                ),
            ),
            "truncated" => (200, event(json!({"content":"partial"}), "length")),
            "empty" => (200, "data: {\"choices\":[]}\n\ndata: [DONE]\n\n".into()),
            "rate_limit" => (429, KEY.into()),
            "server_error" => (500, KEY.into()),
            "oversize" => (200, "x".repeat(1_048_577)),
            _ => panic!("unknown fixture mode"),
        };
        let _ = write!(
            socket,
            "HTTP/1.1 {status} Test\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
    });
    (config, tx, handle)
}
#[tokio::test]
async fn eight_model_failure_contracts_preserve_raw_search_and_retry() {
    let cases: Vec<Fault> =
        serde_json::from_str(include_str!("fixtures/organization_failures.json")).unwrap();
    assert_eq!(cases.len(), 8);
    for case in cases {
        let temp = tempfile::tempdir().unwrap();
        let s = MemoryStore::open(temp.path()).unwrap();
        let raw = capture(&s);
        let before = counts(&s);
        let mut task = s.claim_organization().unwrap().unwrap();
        s.prepare_organization(&mut task).unwrap();
        let (config, release, server) = endpoint(&case.mode);
        config.save(&s.model_config_path()).unwrap();
        memivy_core::model::tools::save_capabilities(
            temp.path(),
            &config,
            &memivy_core::model::tools::Capabilities {
                structured_json: true,
                streaming_text: true,
                single_tool: true,
                multi_turn: true,
            },
        )
        .unwrap();
        let response = if case.mode == "timeout" {
            tokio::time::timeout(
                Duration::from_millis(50),
                s.run_organization(&config, &task),
            )
            .await
            .unwrap_or(Err(ProbeError::Network))
        } else {
            s.run_organization(&config, &task).await
        };
        let _ = release.send(());
        server.join().unwrap();
        let actual = match response {
            Ok(_) => panic!("fault unexpectedly completed"),
            Err(e) => {
                assert!(!format!("{e:?} {e}").contains(KEY));
                match e {
                    ProbeError::Network => "network".into(),
                    ProbeError::InvalidResponse | ProbeError::Truncated => {
                        "invalid_response".into()
                    }
                    ProbeError::TooLarge => "too_large".into(),
                    ProbeError::Status(n) => format!("status_{n}"),
                    _ => panic!("unexpected error"),
                }
            }
        };
        assert_eq!(actual, case.expected, "{}: {}", case.id, case.reason);
        assert_eq!(counts(&s), before);
        s.fail_organization(&task.attempt_id, "unavailable")
            .unwrap();
        assert_eq!(s.capture_by_id(&raw.id).unwrap().text, raw.text);
        assert_eq!(
            s.library(&LibraryQuery {
                query: "SQLite 中文".into(),
                ..Default::default()
            })
            .unwrap()
            .items
            .len(),
            1
        );
        s.retry_organization(&task.memory.memory_id).unwrap();
        let retry = s.claim_organization().unwrap().unwrap();
        let p = proposal(&retry);
        let receipt = s.apply_organization(&retry, &p).unwrap();
        assert_eq!(s.apply_organization(&retry, &p).unwrap(), receipt);
        assert_eq!(counts(&s), vec![1, 1, 2, 2, 0]);
        let export = temp.path().join("article.md");
        s.export_record_markdown(
            &RecordKey {
                kind: "memory".into(),
                id: receipt.memory_id.unwrap(),
            },
            receipt.after_version.as_deref(),
            &export,
        )
        .unwrap();
        let backup = temp.path().join("backup.sqlite3");
        s.backup(&backup).unwrap();
        for file in [export, backup, s.database_path()] {
            assert!(!String::from_utf8_lossy(&std::fs::read(file).unwrap()).contains(KEY));
        }
        s.check_integrity().unwrap();
        println!("{} passed", case.id);
    }
}
#[test]
fn organization_failure_after_receipt_and_index_updates_rolls_back_every_effect() {
    let temp = tempfile::tempdir().unwrap();
    let s = MemoryStore::open(temp.path()).unwrap();
    capture(&s);
    let task = s.claim_organization().unwrap().unwrap();
    let before = counts(&s);
    let db = rusqlite::Connection::open(s.database_path()).unwrap();
    db.execute_batch("CREATE TRIGGER inject_late_failure BEFORE UPDATE ON organization_jobs WHEN NEW.status='done' BEGIN SELECT RAISE(ABORT,'synthetic_api_key_never_export_47291'); END;").unwrap();
    let e = s.apply_organization(&task, &proposal(&task)).unwrap_err();
    assert_eq!(e, DataError::Database);
    assert!(!format!("{e:?} {e}").contains(KEY));
    assert_eq!(counts(&s), before);
    assert_eq!(
        s.memory(&task.memory.memory_id).unwrap().current.id,
        task.memory.id
    );
    assert!(
        s.library(&LibraryQuery {
            query: "回归".into(),
            ..Default::default()
        })
        .unwrap()
        .items
        .is_empty()
    );
    db.execute_batch("DROP TRIGGER inject_late_failure")
        .unwrap();
    let p = proposal(&task);
    let r = s.apply_organization(&task, &p).unwrap();
    assert_eq!(s.apply_organization(&task, &p).unwrap(), r);
    assert_eq!(counts(&s), vec![1, 1, 2, 2, 0]);
    s.check_integrity().unwrap();
}
