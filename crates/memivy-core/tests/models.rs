use memivy_core::{
    model::ModelConfig,
    models::{self, Binding, ModelSettings, Source},
};
use serde_json::json;
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
        provider: Default::default(),
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
    assert!(!error.to_string().contains("test-secret"));
    assert!(!error.to_string().contains("private-memory"));
    assert_eq!(error, models::Error::Authentication);
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
    let mut r = ModelSettings::with_legacy(d.path(), &legacy).unwrap();
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
    assert_eq!(
        r.save(d.path(), "initial").unwrap_err(),
        models::Error::Conflict
    );
    let mut copy = ModelSettings::read(d.path()).unwrap();
    copy.llm = None;
    let revision = copy.revision.clone();
    copy.save(d.path(), &revision).unwrap();
    assert!(ModelSettings::read(d.path()).unwrap().llm_config().is_err());
}
#[test]
fn embedding_identity_excludes_key_but_includes_encoding() {
    let mut r = ModelSettings {
        embedding: Binding {
            source: Source::Service,
            base_url: "http://localhost:1234/v1".into(),
            model: "first".into(),
            dimensions: Some(3),
            ..Binding::default()
        },
        ..ModelSettings::default()
    };
    let first = r.fingerprint().unwrap();
    r.embedding.api_key = Some("new-secret".into());
    assert_eq!(r.fingerprint().unwrap(), first);
    r.embedding.model = "second".into();
    assert_ne!(r.fingerprint().unwrap(), first);
}
#[test]
fn disabled_organization_does_not_enqueue_new_captures() {
    let d = tempfile::tempdir().unwrap();
    let store = memivy_core::memory::MemoryStore::open(d.path()).unwrap();
    let mut r = ModelSettings {
        auto_organize: false,
        ..ModelSettings::default()
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
    let mut current = ModelSettings::default();
    current.save(dir.path(), "initial").unwrap();
    let mut candidate = current.clone();
    candidate.embedding.model = "candidate marker".into();
    {
        let lock = embedding::lock(dir.path(), "embedding-control.lock").unwrap();
        assert!(
            store
                .apply_embedding_model(&mut candidate, &current.revision)
                .is_err()
        );
        assert_eq!(
            ModelSettings::read(dir.path()).unwrap().revision,
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
        ModelSettings::read(dir.path()).unwrap().revision,
        current.revision
    );
    Preferences::default().save(dir.path()).unwrap();
    // In-flight work may finish with its old snapshot; it must not block switching.
    let _writer = embedding::lock(dir.path(), "embedding-writer.lock").unwrap();
    store
        .apply_embedding_model(&mut candidate, &current.revision)
        .unwrap();
    assert_eq!(
        ModelSettings::read(dir.path()).unwrap().embedding.model,
        "candidate marker"
    );
    assert!(Preferences::read(dir.path()).unwrap().preparing);
}

#[test]
fn oversized_registry_cannot_replace_a_readable_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let mut current = ModelSettings::default();
    current.save(dir.path(), "initial").unwrap();
    let mut candidate = current.clone();
    candidate.voice.base_url = "x".repeat(256 * 1024);
    assert_eq!(
        candidate.save(dir.path(), &current.revision).unwrap_err(),
        models::Error::ConfigurationTooLarge
    );
    assert_eq!(
        ModelSettings::read(dir.path()).unwrap().revision,
        current.revision
    );
}

#[test]
fn service_status_codes_are_stable_and_never_contain_server_content() {
    for (status, expected) in [
        (403, models::Error::Authentication),
        (429, models::Error::RateLimit),
        (503, models::Error::HttpStatus),
    ] {
        let (url, h) = server("private-memory secret-key", status);
        let error =
            models::embed(&model(url), "query", Some(3), Duration::from_secs(2)).unwrap_err();
        assert_eq!(error, expected);
        assert!(!error.to_string().contains("secret-key"));
        h.join().unwrap();
    }
}

#[test]
fn flat_settings_import_credentials_before_voice_rewrites_its_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("voice")).unwrap();
    let old = json!({
        "revision":"old-revision", "auto_organize":false,
        "connections":[
            {"id":"active","name":"Active","base_url":"http://localhost:1234/v1","api_key":"active-secret"},
            {"id":"retained","name":"Retained","base_url":"http://localhost:4321/v1","api_key":"retained-secret"}
        ],
        "llm":{"source":"service","connection":"active","model":"chat-model"},
        "embedding":{"source":"service","connection":"active","model":"embedding-model","dimensions":3},
        "voice":{"source":"service","connection":"active","model":"new-asr"}
    });
    let session = json!({"id":"recording-id", "endpoint":"http://localhost:4321/v1",
        "binding":{"source":"service","connection":"retained","model":"old-asr"},
        "parts":[{"file":"recording.pcm","text":null}],"error":"voice_network"});
    memivy_core::embedding::write_json(&dir.path().join("models.json"), &old).unwrap();
    memivy_core::embedding::write_json(&dir.path().join("voice/session.json"), &session).unwrap();
    let before = std::fs::read(dir.path().join("models.json")).unwrap();
    let session_before = std::fs::read(dir.path().join("voice/session.json")).unwrap();
    let mut settings = ModelSettings::read(dir.path()).unwrap();
    assert_eq!(
        settings.llm_config().unwrap().api_key.as_deref(),
        Some("active-secret")
    );
    assert_eq!(settings.embedding.api_key.as_deref(), Some("active-secret"));
    assert_eq!(
        settings
            .voice_config("recording-id", "http://localhost:4321/v1", "old-asr")
            .unwrap()
            .api_key
            .as_deref(),
        Some("retained-secret")
    );
    assert_ne!(
        std::fs::read(dir.path().join("models.json")).unwrap(),
        before
    );
    // Voice can now persist a flat, key-free snapshot before another config edit.
    let mut rewritten = session.clone();
    rewritten["binding"] =
        json!({"source":"service","model":"old-asr","base_url":"http://localhost:4321/v1"});
    memivy_core::embedding::write_json(&dir.path().join("voice/session.json"), &rewritten).unwrap();
    assert_eq!(
        ModelSettings::read(dir.path())
            .unwrap()
            .voice_config("recording-id", "http://localhost:4321/v1", "old-asr")
            .unwrap()
            .api_key
            .as_deref(),
        Some("retained-secret")
    );
    memivy_core::embedding::write_json(&dir.path().join("voice/session.json"), &session).unwrap();
    settings.save(dir.path(), "old-revision").unwrap();
    let flat: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("models.json")).unwrap()).unwrap();
    assert!(flat.get("connections").is_none());
    assert_eq!(flat["format_version"], 2);
    assert!(flat["voice"].get("connection").is_none());
    let reopened = ModelSettings::read(dir.path()).unwrap();
    assert!(!reopened.auto_organize);
    assert_eq!(reopened.voice.model, "new-asr");
    assert_eq!(
        reopened
            .voice_config("recording-id", "http://localhost:4321/v1", "old-asr")
            .unwrap()
            .api_key
            .as_deref(),
        Some("retained-secret")
    );
    assert!(
        reopened
            .voice_config("another-recording", "http://localhost:4321/v1", "old-asr")
            .is_err()
    );
    assert_eq!(
        std::fs::read(dir.path().join("voice/session.json")).unwrap(),
        session_before
    );
}

#[test]
fn unresolved_old_connections_and_unknown_formats_fail_without_overwriting_config() {
    let dir = tempfile::tempdir().unwrap();
    for value in [
        json!({"revision":"old","connections":[],"voice":{"source":"service","connection":"missing","model":"asr"}}),
        json!({"format_version":99,"revision":"future"}),
        json!({"format_version":2,"voice":{"source":"service","model":"asr"}}),
    ] {
        memivy_core::embedding::write_json(&dir.path().join("models.json"), &value).unwrap();
        let before = std::fs::read(dir.path().join("models.json")).unwrap();
        assert!(ModelSettings::read(dir.path()).is_err());
        assert_eq!(
            ModelSettings::default()
                .save(dir.path(), "initial")
                .unwrap_err(),
            ModelSettings::read(dir.path()).err().unwrap()
        );
        assert_eq!(
            std::fs::read(dir.path().join("models.json")).unwrap(),
            before
        );
    }
}

#[test]
fn unreadable_voice_state_preserves_old_private_config_without_blocking_active_models() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("voice")).unwrap();
    let old = json!({"revision":"old","connections":[{"id":"a","name":"A","base_url":"http://localhost:1234/v1","api_key":"private-key"}],
        "llm":{"source":"service","connection":"a","model":"chat"},"embedding":{"source":"local"},
        "voice":{"source":"service","connection":"a","model":"asr"}});
    memivy_core::embedding::write_json(&dir.path().join("models.json"), &old).unwrap();
    std::fs::write(
        dir.path().join("voice/session.json"),
        b"broken optional session",
    )
    .unwrap();
    let before = std::fs::read(dir.path().join("models.json")).unwrap();
    let mut settings = ModelSettings::read(dir.path()).unwrap();
    assert_eq!(settings.llm_config().unwrap().model, "chat");
    assert!(settings.save(dir.path(), "old").is_err());
    assert_eq!(
        std::fs::read(dir.path().join("models.json")).unwrap(),
        before
    );
    // Replacing unusable voice state with a new recording unblocks the import.
    let session = json!({"id":"new-recording","endpoint":"http://localhost:1234/v1",
        "binding":{"source":"service","base_url":"http://localhost:1234/v1","model":"asr"}});
    memivy_core::embedding::write_json(&dir.path().join("voice/session.json"), &session).unwrap();
    let converted = ModelSettings::read(dir.path()).unwrap();
    assert_eq!(
        converted
            .voice_config("new-recording", "http://localhost:1234/v1", "asr")
            .unwrap()
            .api_key
            .as_deref(),
        Some("private-key")
    );
    assert!(
        !std::fs::read_to_string(dir.path().join("models.json"))
            .unwrap()
            .contains("connections")
    );
}
