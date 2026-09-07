use memivy_core::{memory::*, model::ModelConfig};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};
use uuid::Uuid;
fn id() -> String {
    Uuid::new_v4().to_string()
}
type Fixture = (
    ModelConfig,
    Arc<Mutex<Vec<Value>>>,
    mpsc::Receiver<()>,
    mpsc::Sender<()>,
    std::thread::JoinHandle<()>,
);
fn fixture(block_answer: bool, unknown: bool) -> Fixture {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let config = ModelConfig {
        base_url: format!("http://{}/v1", listener.local_addr().unwrap()),
        model: "synthetic-only".into(),
        api_key: None,
        disable_reasoning: false,
    };
    let requests = Arc::new(Mutex::new(vec![]));
    let log = requests.clone();
    let (arrived_tx, arrived_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        for n in 0..2 {
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
                if let Some(p) = bytes.windows(4).position(|x| x == b"\r\n\r\n") {
                    break p + 4;
                }
            };
            let headers = String::from_utf8_lossy(&bytes[..end]);
            let size: usize = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|x| x.trim().parse().unwrap())
                })
                .unwrap();
            while bytes.len() < end + size {
                let mut part = [0; 4096];
                let len = socket.read(&mut part).unwrap();
                assert!(len > 0);
                bytes.extend_from_slice(&part[..len]);
            }
            let request: Value = serde_json::from_slice(&bytes[end..end + size]).unwrap();
            log.lock().unwrap().push(request);
            if n == 1 && block_answer {
                arrived_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            }
            let content = if n == 0 {
                json!({"queries":["桌面体验"]})
            } else {
                json!({"recollection":"旧版本说先做好桌面体验。","ideas":"generated_suggestion_not_memory","sources":[if unknown{"M99"}else{"M1"}],"conclusion":"reviewed_conclusion_fixture"})
            };
            let body=json!({"choices":[{"message":{"content":content.to_string()},"finish_reason":"stop"}]}).to_string();
            write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
    });
    (config, requests, arrived_rx, release_tx, server)
}
fn setup() -> (tempfile::TempDir, MemoryStore, Receipt, SourceRef, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    let raw = store
        .capture(&CaptureRequest {
            request_id: id(),
            text: "原话：桌面体验优先".into(),
            origin: Origin::User {
                app: "Test".into(),
                project: None,
                uri: None,
            },
        })
        .unwrap();
    let receipt = store
        .apply_capture(&ChangeRequest {
            request_id: id(),
            capture_id: raw.id,
            destination: Destination::New,
            title: "桌面体验".into(),
            body: "旧版本：桌面体验优先".into(),
            actor: Actor::User,
        })
        .unwrap();
    let source = SourceRef::Version(receipt.after_version.clone().unwrap());
    let topic = id();
    store.create_conversation(&topic, "继续讨论").unwrap();
    (dir, store, receipt, source, topic)
}
#[tokio::test]
async fn grounded_discussion_uses_versioned_sources_and_saves_only_after_confirmation() {
    let (_dir, store, receipt, source, topic) = setup();
    let turn = store
        .start_turn(
            &id(),
            &topic,
            "接着想桌面体验",
            std::slice::from_ref(&source),
        )
        .unwrap();
    store
        .edit_memory(&EditRequest {
            request_id: id(),
            memory_id: receipt.memory_id.unwrap(),
            expected_version: receipt.after_version.unwrap(),
            title: "新版标题".into(),
            body: "当前版本已经改了".into(),
        })
        .unwrap();
    let (config, requests, _, _, server) = fixture(false, false);
    store
        .answer_discussion(&config, &topic, &turn, std::slice::from_ref(&source))
        .await
        .unwrap();
    server.join().unwrap();
    let result = store.turn(&turn.id).unwrap();
    assert_eq!(result.assistant.status, "complete");
    assert_eq!(result.assistant.citations[0].source, source);
    let calls = requests.lock().unwrap();
    assert_eq!(calls.len(), 2);
    let payload: Value =
        serde_json::from_str(calls[1]["messages"][1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(payload["evidence"][0]["text"], "旧版本：桌面体验优先");
    assert!(payload["evidence"].as_array().unwrap().len() <= 8);
    drop(calls);
    assert!(
        store
            .search("generated_suggestion_not_memory", 20)
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .search("reviewed_conclusion_fixture", 20)
            .unwrap()
            .is_empty()
    );
    let confirmed = store
        .save_conclusion(&ConclusionRequest {
            request_id: id(),
            message_id: result.assistant.id,
            destination: Destination::New,
            title: "我确认的结论".into(),
            text: "reviewed_conclusion_fixture".into(),
        })
        .unwrap();
    assert!(confirmed.memory_id.is_some());
    assert_eq!(
        store
            .search("reviewed_conclusion_fixture", 20)
            .unwrap()
            .len(),
        1
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_deleted_and_unknown_evidence_never_complete_late() {
    for outcome in ["cancel", "deleted", "unknown"] {
        let (_dir, store, receipt, source, topic) = setup();
        let turn = store
            .start_turn(&id(), &topic, "接着想", std::slice::from_ref(&source))
            .unwrap();
        let (config, _, arrived, release, server) =
            fixture(outcome != "unknown", outcome == "unknown");
        let worker_store = store.clone();
        let turn_id = turn.id.clone();
        let task = tokio::spawn(async move {
            worker_store
                .answer_discussion(&config, &topic, &turn, &[source])
                .await
        });
        if outcome != "unknown" {
            tokio::task::spawn_blocking(move || {
                arrived.recv_timeout(Duration::from_secs(10)).unwrap()
            })
            .await
            .unwrap();
            if outcome == "cancel" {
                store.cancel_turn(&turn_id).unwrap();
            } else {
                store
                    .trash_memory(
                        receipt.memory_id.as_ref().unwrap(),
                        receipt.after_version.as_ref().unwrap(),
                    )
                    .unwrap();
            }
            release.send(()).unwrap();
        }
        let result = task.await.unwrap();
        server.join().unwrap();
        if let Err(failure) = result {
            store.fail_turn(&turn_id, failure).unwrap();
        }
        let completed = store.turn(&turn_id).unwrap();
        assert_ne!(completed.assistant.status, "complete");
        assert!(completed.assistant.text.is_empty());
        assert!(
            store
                .search("generated_suggestion_not_memory", 20)
                .unwrap()
                .is_empty()
        );
    }
}
#[test]
fn discussion_drafts_and_pins_survive_restart_and_history_reads_from_both_ends() {
    let (dir, store, _, source, topic) = setup();
    let draft = WorkspaceDraft {
        key: format!("discussion:{topic}"),
        request_id: id(),
        title: String::new(),
        body: "还没问的问题".into(),
        expected_version: None,
        context: vec![source.clone()],
    };
    store.save_workspace_draft(&draft).unwrap();
    let reopened = MemoryStore::open(dir.path()).unwrap();
    let loaded = reopened.workspace_draft(&draft.key).unwrap().unwrap();
    assert_eq!(loaded.context, vec![source]);
    assert_eq!(loaded.body, draft.body);
    assert!(reopened.search(&draft.body, 20).unwrap().is_empty());
    for n in 0..25 {
        let t = store
            .start_turn(&id(), &topic, &format!("问题{n}"), &[])
            .unwrap();
        store.finish_turn(&t.id, &format!("建议{n}"), &[]).unwrap();
    }
    let latest = store.recent_messages(&topic, None, 40).unwrap();
    assert_eq!(latest.len(), 40);
    assert_eq!(latest.last().unwrap().text, "建议24");
    let older = store
        .recent_messages(&topic, Some(latest[0].seq), 40)
        .unwrap();
    assert_eq!(older.len(), 10);
    assert!(older.last().unwrap().seq < latest[0].seq);
}
