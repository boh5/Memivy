use memivy_core::{
    model::ModelConfig,
    models::{self, Binding, Connection, Registry, Source},
};
use std::{
    io::{Read, Write},
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    time::Duration,
};
fn server(body: &str, status: u16) -> (String, std::thread::JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let body = body.to_owned();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut req = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = stream.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            req.extend_from_slice(&buf[..n]);
            if let Some(end) = req.windows(4).position(|x| x == b"\r\n\r\n") {
                let header = String::from_utf8_lossy(&req[..end]).to_ascii_lowercase();
                let len = header
                    .lines()
                    .find_map(|l| {
                        l.strip_prefix("content-length:")
                            .and_then(|s| s.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if req.len() >= end + 4 + len {
                    break;
                }
            }
        }
        write!(stream,"HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        req
    });
    (url, handle)
}
fn model(url: String) -> ModelConfig {
    ModelConfig {
        base_url: url,
        model: "synthetic".into(),
        api_key: Some("test-secret".into()),
        disable_reasoning: false,
        max_output_tokens: None,
        output_token_parameter: Default::default(),
    }
}
#[test]
fn embedding_normalizes_and_verifies_dimension() {
    let (url, h) = server(r#"{"data":[{"index":0,"embedding":[3,4,0]}]}"#, 200);
    let v = models::embed(
        &model(url),
        "synthetic note",
        Some(3),
        Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(v, vec![0.6, 0.8, 0.]);
    let req = String::from_utf8(h.join().unwrap()).unwrap();
    assert!(req.starts_with("POST /v1/embeddings"));
    assert!(req.contains("test-secret"));
    assert!(req.contains("synthetic note"));
    for response in [
        r#"{"data":[{"index":0,"embedding":[1,0]}]}"#,
        r#"{"data":[{"index":0,"embedding":[0,0,0]}]}"#,
        r#"{"data":[{"index":2,"embedding":[1,0,0]}]}"#,
    ] {
        let (url, h) = server(response, 200);
        assert!(models::embed(&model(url), "query", Some(3), Duration::from_secs(2)).is_err());
        h.join().unwrap();
    }
}
#[test]
fn transcription_sends_wav_and_reads_text() {
    let (url, h) = server(r#"{"text":"测试转写 hello"}"#, 200);
    assert_eq!(
        models::transcribe(&model(url), &vec![0.1; 1600]).unwrap(),
        "测试转写 hello"
    );
    let req = h.join().unwrap();
    let s = String::from_utf8_lossy(&req);
    assert!(s.starts_with("POST /v1/audio/transcriptions"));
    assert!(s.contains("multipart/form-data"));
    assert!(s.contains("filename=\"segment.wav\""));
    assert!(s.contains("RIFF"));
    assert!(s.contains("WAVEfmt "));
}
#[test]
fn server_errors_never_echo_credentials_or_response_body() {
    let (url, h) = server(r#"{"error":"test-secret private-memory"}"#, 401);
    let error = models::transcribe(&model(url), &[0.; 160]).unwrap_err();
    assert!(!error.contains("test-secret"));
    assert!(!error.contains("private-memory"));
    assert!(error.contains("认证"));
    h.join().unwrap();
    let (url, h) = server(r#"{"text":42}"#, 200);
    assert!(models::transcribe(&model(url), &[0.; 160]).is_err());
    h.join().unwrap();
}
#[test]
fn registry_preserves_legacy_secrets_and_rejects_stale_writes() {
    let d = tempfile::tempdir().unwrap();
    let legacy = d.path().join("model.json");
    model("http://localhost:1234/v1".into())
        .save(&legacy)
        .unwrap();
    let mut r = Registry::with_legacy(d.path(), &legacy).unwrap();
    assert_eq!(
        r.llm_config().unwrap().api_key.as_deref(),
        Some("test-secret")
    );
    let original = std::fs::read(&legacy).unwrap();
    r.save(d.path(), "initial").unwrap();
    assert_eq!(std::fs::read(&legacy).unwrap(), original);
    assert_eq!(
        std::fs::metadata(d.path().join("models.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(r.save(d.path(), "initial").is_err());
    let mut copy = Registry::read(d.path()).unwrap();
    copy.llm = None;
    let revision = copy.revision.clone();
    copy.save(d.path(), &revision).unwrap();
    assert!(Registry::read(d.path()).unwrap().llm_config().is_err());
}
#[test]
fn embedding_identity_excludes_key_and_name_but_includes_encoding() {
    let mut r = Registry::default();
    r.connections.push(Connection {
        id: "one".into(),
        name: "First".into(),
        base_url: "http://localhost:1234/v1".into(),
        api_key: None,
    });
    r.embedding = Binding {
        source: Source::Service,
        connection: "one".into(),
        model: "first".into(),
        dimensions: Some(3),
        ..Binding::default()
    };
    let first = r.fingerprint().unwrap();
    r.connections[0].api_key = Some("new-secret".into());
    r.connections[0].name = "Renamed".into();
    assert_eq!(r.fingerprint().unwrap(), first);
    r.embedding.query_prefix = "query: ".into();
    assert_ne!(r.fingerprint().unwrap(), first);
}
#[test]
fn disabled_organization_does_not_enqueue_new_captures() {
    let d = tempfile::tempdir().unwrap();
    let store = memivy_core::memory::MemoryStore::open(d.path()).unwrap();
    let mut r = Registry {
        auto_organize: false,
        ..Registry::default()
    };
    r.save(d.path(), "initial").unwrap();
    store
        .capture(&memivy_core::memory::CaptureRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            text: "关闭期间保留原话".into(),
            origin: memivy_core::memory::Origin::User {
                app: "QA".into(),
                project: None,
                uri: None,
            },
        })
        .unwrap();
    let rev = r.revision.clone();
    r.auto_organize = true;
    r.save(d.path(), &rev).unwrap();
    assert!(store.claim_organization().unwrap().is_none());
}

#[test]
fn embedding_activation_rejects_busy_or_clearing_state_before_publishing() {
    use memivy_core::embedding::{self, Preferences};
    let dir = tempfile::tempdir().unwrap();
    let store = memivy_core::memory::MemoryStore::open(dir.path()).unwrap();
    let mut current = Registry::default();
    current.save(dir.path(), "initial").unwrap();
    let mut candidate = current.clone();
    candidate.embedding.query_prefix = "candidate marker".into();
    {
        let lock = embedding::lock(dir.path(), "embedding-control.lock").unwrap();
        assert!(
            store
                .apply_embedding_model(&mut candidate, &current.revision)
                .is_err()
        );
        assert_eq!(
            Registry::read(dir.path()).unwrap().revision,
            current.revision
        );
        drop(lock);
    }
    Preferences {
        clear_requested: true,
        ..Preferences::default()
    }
    .save(dir.path())
    .unwrap();
    assert!(
        store
            .apply_embedding_model(&mut candidate, &current.revision)
            .is_err()
    );
    assert_eq!(
        Registry::read(dir.path()).unwrap().revision,
        current.revision
    );
    Preferences::default().save(dir.path()).unwrap();
    // In-flight work may finish with its old snapshot; it must not block switching.
    let _writer = embedding::lock(dir.path(), "embedding-writer.lock").unwrap();
    store
        .apply_embedding_model(&mut candidate, &current.revision)
        .unwrap();
    assert_eq!(
        Registry::read(dir.path()).unwrap().embedding.query_prefix,
        "candidate marker"
    );
    assert!(Preferences::read(dir.path()).unwrap().preparing);
}

#[test]
fn oversized_registry_cannot_replace_a_readable_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let mut current = Registry::default();
    current.save(dir.path(), "initial").unwrap();
    let mut candidate = current.clone();
    candidate.connections.push(Connection {
        id: "qa".into(),
        name: "QA".into(),
        base_url: format!("http://localhost:1234/v1/{}", "x".repeat(256 * 1024)),
        api_key: None,
    });
    assert!(
        candidate
            .save(dir.path(), &current.revision)
            .unwrap_err()
            .contains("过大")
    );
    assert_eq!(
        Registry::read(dir.path()).unwrap().revision,
        current.revision
    );
}
