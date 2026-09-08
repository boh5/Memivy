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
    s.capture(&CaptureRequest {
        request_id: id(),
        text: " \n故障中的原话 SQLite 中文\n ".into(),
        origin: Origin::User {
            app: "fixture".into(),
            project: None,
            uri: None,
        },
    })
    .unwrap()
}
fn proposal() -> OrganizationProposal {
    OrganizationProposal {
        action: "new".into(),
        target: String::new(),
        title: "故障恢复".into(),
        addition: "故障中的原话 SQLite 中文".into(),
        changes: vec![],
        keywords: vec!["回归".into()],
        reason: "合成故障测试".into(),
    }
}
fn counts(s: &MemoryStore) -> Vec<i64> {
    let db = rusqlite::Connection::open(s.database_path()).unwrap();
    [
        "captures",
        "memories",
        "memory_versions",
        "receipts",
        "capture_keywords",
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
        let mut p = proposal();
        p.action = "append".into();
        p.target = "M99".into();
        p.title = String::new();
        let (status,body)=match mode.as_str(){
            "unknown_target"=>(200,json!({"choices":[{"finish_reason":"tool_calls","message":{"tool_calls":[{"type":"function","function":{"name":"update_memory","arguments":json!({"target":p.target,"addition":p.addition,"changes":p.changes,"keywords":p.keywords,"reason":p.reason}).to_string()}}]}}]}).to_string()),
            "truncated"=>(200,json!({"choices":[{"finish_reason":"length","message":{"content":"{\"action\":"}}]}).to_string()),
            "empty"=>(200,json!({"choices":[]}).to_string()),
            "rate_limit"=>(429,KEY.into()),"server_error"=>(500,KEY.into()),"oversize"=>(200,"x".repeat(65537)),_=>panic!("unknown fixture mode")};
        let _ = write!(
            socket,
            "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
    });
    (config, tx, handle)
}
#[tokio::test]
async fn eight_formal_model_failure_contracts_preserve_raw_search_and_retry() {
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
        let response = s.propose_organization(&config, &task).await;
        let _ = release.send(());
        server.join().unwrap();
        let actual = match response {
            Ok(p) => {
                assert_eq!(
                    s.apply_organization(&task, &p).unwrap_err(),
                    DataError::Invalid
                );
                "invalid".into()
            }
            Err(e) => {
                assert!(!format!("{e:?} {e}").contains(KEY));
                match e {
                    ProbeError::Network => "network".into(),
                    ProbeError::InvalidResponse => "invalid_response".into(),
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
        s.retry_organization(&raw.id).unwrap();
        let retry = s.claim_organization().unwrap().unwrap();
        let p = proposal();
        let receipt = s.apply_organization(&retry, &p).unwrap();
        assert_eq!(s.apply_organization(&retry, &p).unwrap(), receipt);
        assert_eq!(counts(&s), vec![1, 1, 1, 1, 1]);
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
    let raw = capture(&s);
    let task = s.claim_organization().unwrap().unwrap();
    let before = counts(&s);
    let db = rusqlite::Connection::open(s.database_path()).unwrap();
    db.execute_batch("CREATE TRIGGER inject_late_failure BEFORE UPDATE ON organization_jobs WHEN NEW.status='done' BEGIN SELECT RAISE(ABORT,'synthetic_api_key_never_export_47291'); END;").unwrap();
    let e = s.apply_organization(&task, &proposal()).unwrap_err();
    assert_eq!(e, DataError::Database);
    assert!(!format!("{e:?} {e}").contains(KEY));
    assert_eq!(counts(&s), before);
    assert_eq!(s.capture_by_id(&raw.id).unwrap().understanding, "pending");
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
    let p = proposal();
    let r = s.apply_organization(&task, &p).unwrap();
    assert_eq!(s.apply_organization(&task, &p).unwrap(), r);
    assert_eq!(counts(&s), vec![1, 1, 1, 1, 1]);
    s.check_integrity().unwrap();
}
