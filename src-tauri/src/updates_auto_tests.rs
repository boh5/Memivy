//! Verify update scheduling decisions against real signed local update responses.
use super::*;
use crate::updates_tests::{cli, serve};
use std::{
    io::{Read, Write},
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    path::Path,
};
use tauri::test::MockRuntime;

struct SignedUpdate {
    dir: tempfile::TempDir,
    bytes: Vec<u8>,
    public: String,
    signature: String,
}
impl SignedUpdate {
    fn new() -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let key = dir.path().join("test.key");
        let payload = dir.path().join("synthetic.bin");
        let bytes = b"synthetic automatic update archive".to_vec();
        std::fs::write(&payload, &bytes).unwrap();
        cli(
            root,
            &["signer", "generate", "--ci", "-w", key.to_str().unwrap()],
        );
        cli(
            root,
            &[
                "signer",
                "sign",
                "-f",
                key.to_str().unwrap(),
                "-p",
                "",
                payload.to_str().unwrap(),
            ],
        );
        let public = std::fs::read_to_string(dir.path().join("test.key.pub")).unwrap();
        let signature = std::fs::read_to_string(dir.path().join("synthetic.bin.sig")).unwrap();
        Self {
            dir,
            bytes,
            public,
            signature,
        }
    }
    fn manifest(&self, download_url: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "version": "99.0.0", "notes": "Synthetic automatic update", "platforms": {
                "darwin-aarch64": { "url": download_url, "signature": self.signature.trim() }
            }
        }))
        .unwrap()
    }
    fn app(&self, endpoint: &str) -> tauri::App<MockRuntime> {
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context.config_mut().plugins.0.insert(
            "updater".into(),
            serde_json::json!({
                "pubkey": self.public.trim(), "endpoints": [endpoint],
                "dangerousInsecureTransportProtocol": true
            }),
        );
        tauri::test::mock_builder()
            .manage(Updates::new(
                "0.1.5".into(),
                self.dir.path().join("updates.json"),
            ))
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(context)
            .unwrap()
    }
}

struct PausedResponse {
    url: String,
    requested: tokio::sync::oneshot::Receiver<()>,
    resume: std::sync::mpsc::Sender<()>,
    server: std::thread::JoinHandle<()>,
}
fn serve_paused(response: Vec<u8>) -> PausedResponse {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/latest.json", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let (requested_tx, requested) = tokio::sync::oneshot::channel();
    let (resume, resumed) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let start = std::time::Instant::now();
        let mut stream = loop {
            if let Ok((stream, _)) = listener.accept() {
                break stream;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "Expected update request"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = [0; 4096];
        assert!(stream.read(&mut request).unwrap() > 0);
        requested_tx.send(()).unwrap();
        resumed.recv_timeout(Duration::from_secs(5)).unwrap();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.len()
        )
        .unwrap();
        stream.write_all(&response).unwrap();
    });
    PausedResponse {
        url,
        requested,
        resume,
        server,
    }
}

#[test]
fn automatic_preference_defaults_persists_privately_and_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("updates.json");
    let initial = Updates::new("0.1.5".into(), path.clone());
    assert!(initial.inner.lock().unwrap().status.automatic);
    initial.set_automatic(false).unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let restored = Updates::new("0.1.5".into(), path.clone());
    assert!(!restored.inner.lock().unwrap().status.automatic);
    restored.set_automatic(true).unwrap();
    assert!(
        Updates::new("0.1.5".into(), path.clone())
            .inner
            .lock()
            .unwrap()
            .status
            .automatic
    );
    for invalid in ["invalid", "{}", r#"{"automatic":"false"}"#] {
        std::fs::write(&path, invalid).unwrap();
        let invalid = Updates::new("0.1.5".into(), path.clone());
        let status = invalid.inner.lock().unwrap().status.clone();
        assert!(!status.automatic);
        assert_eq!(status.error, Some("update_preferences_unavailable"));
    }
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    let unreadable = Updates::new("0.1.5".into(), path.clone());
    assert!(!unreadable.inner.lock().unwrap().status.automatic);
    assert_eq!(
        unreadable.set_automatic(true).unwrap_err().code,
        "update_preferences_save_failed"
    );
    assert!(!unreadable.inner.lock().unwrap().status.automatic);
}

#[tokio::test]
async fn automatic_tick_downloads_verified_bytes_preserves_ready_and_retries_tampering() {
    let fixture = SignedUpdate::new();
    let (download_url, download_server) =
        serve(vec![b"tampered archive".to_vec(), fixture.bytes.clone()]);
    let (url, check_server) = serve(vec![
        fixture.manifest(&download_url),
        fixture.manifest(&download_url),
    ]);
    let app = fixture.app(&url);
    automatic_update(app.handle()).await;
    {
        let updates = app.state::<Updates>();
        let pending = updates.inner.lock().unwrap();
        assert_eq!(pending.status.phase, "available");
        assert_eq!(pending.status.error, Some("update_download_failed"));
        assert!(
            pending.bytes.is_none(),
            "Unverified bytes must never be installable"
        );
    }
    automatic_update(app.handle()).await;
    check_server.join().unwrap();
    download_server.join().unwrap();
    automatic_update(app.handle()).await;
    let updates = app.state::<Updates>();
    let pending = updates.inner.lock().unwrap();
    assert_eq!(
        pending.status.phase, "ready",
        "Periodic checks must preserve a downloaded update"
    );
    assert_eq!(pending.bytes.as_ref(), Some(&fixture.bytes));
    assert!(pending.status.error.is_none());
}

#[tokio::test]
async fn disabled_and_busy_automatic_ticks_do_not_start_network_requests() {
    let dir = tempfile::tempdir().unwrap();
    // No updater plugin is installed: reaching the network builder would panic.
    let app = tauri::test::mock_builder()
        .manage(Updates::new(
            "0.1.5".into(),
            dir.path().join("updates.json"),
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let updates = app.state::<Updates>();
    updates.set_automatic(false).unwrap();
    automatic_update(app.handle()).await;
    assert_eq!(updates.inner.lock().unwrap().status.phase, "idle");
    updates.set_automatic(true).unwrap();
    for phase in [
        "checking",
        "downloading",
        "ready",
        "preparing",
        "installing",
    ] {
        {
            let mut pending = updates.inner.lock().unwrap();
            pending.status.phase = phase;
            pending.bytes = Some(vec![1, 2, 3]);
        }
        automatic_update(app.handle()).await;
        let pending = updates.inner.lock().unwrap();
        assert_eq!(pending.status.phase, phase);
        assert_eq!(pending.bytes, Some(vec![1, 2, 3]));
    }
    assert!(
        !enabled(app.handle()),
        "Development builds must never start the production scheduler"
    );
}

#[tokio::test]
async fn disabling_during_check_prevents_download_and_manual_control_still_works() {
    let fixture = SignedUpdate::new();
    let download = serve_paused(fixture.bytes.clone());
    let check_response = serve_paused(fixture.manifest(&download.url));
    let app = fixture.app(&check_response.url);
    tokio::join!(automatic_update(app.handle()), async {
        check_response.requested.await.unwrap();
        app.state::<Updates>().set_automatic(false).unwrap();
        assert_eq!(
            check(app.handle(), false).await.unwrap_err().code,
            "update_busy"
        );
        check_response.resume.send(()).unwrap();
    });
    check_response.server.join().unwrap();
    {
        let updates = app.state::<Updates>();
        let pending = updates.inner.lock().unwrap();
        assert_eq!(pending.status.phase, "available");
        assert!(!pending.status.automatic);
        assert!(pending.bytes.is_none());
    }
    // A manual download remains allowed with automation off. Other commands may
    // race it, but neither a check nor a second download may replace its state.
    let (result, ()) = tokio::join!(super::download(app.handle(), false), async {
        download.requested.await.unwrap();
        assert_eq!(
            check(app.handle(), false).await.unwrap_err().code,
            "update_busy"
        );
        assert_eq!(
            super::download(app.handle(), false).await.unwrap_err().code,
            "update_busy"
        );
        app.state::<Updates>().set_automatic(true).unwrap();
        automatic_update(app.handle()).await;
        app.state::<Updates>().set_automatic(false).unwrap();
        download.resume.send(()).unwrap();
    });
    result.unwrap();
    download.server.join().unwrap();
    let updates = app.state::<Updates>();
    let pending = updates.inner.lock().unwrap();
    assert_eq!(pending.status.phase, "ready");
    assert_eq!(pending.bytes.as_ref(), Some(&fixture.bytes));
    assert!(!pending.status.automatic);
}

#[test]
fn status_revisions_order_concurrent_events_and_reads() {
    use std::sync::{Arc, mpsc};
    use tauri::Listener;

    let dir = tempfile::tempdir().unwrap();
    let app = tauri::test::mock_builder()
        .manage(Updates::new(
            "0.1.5".into(),
            dir.path().join("updates.json"),
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let resume_rx = Mutex::new(resume_rx);
    let listener_app = app.handle().clone();
    let listener_seen = seen.clone();
    app.listen("update-status", move |event| {
        let payload: serde_json::Value = serde_json::from_str(event.payload()).unwrap();
        let automatic = payload["automatic"].as_bool().unwrap();
        // Synchronous Rust listeners must be able to read the state while an
        // event is in progress without preventing another snapshot.
        let _status = listener_app
            .state::<Updates>()
            .inner
            .lock()
            .unwrap()
            .status
            .clone();
        if automatic {
            entered_tx.send(()).unwrap();
            resume_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
        }
        listener_seen
            .lock()
            .unwrap()
            .push((payload["revision"].as_u64().unwrap(), automatic));
    });
    let first_app = app.handle().clone();
    let first = std::thread::spawn(move || emit(&first_app));
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();

    let (changed_tx, changed_rx) = mpsc::channel();
    let (published_tx, published_rx) = mpsc::channel();
    let second_app = app.handle().clone();
    let second = std::thread::spawn(move || {
        second_app.state::<Updates>().set_automatic(false).unwrap();
        // Model a status command reading the changed preference before its
        // event is emitted. It must already outrank the delayed old event.
        changed_tx
            .send(second_app.state::<Updates>().snapshot())
            .unwrap();
        emit(&second_app);
        published_tx.send(()).unwrap();
    });
    let changed = changed_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    published_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    resume_tx.send(()).unwrap();
    first.join().unwrap();
    second.join().unwrap();
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0], (1, true));
    assert!(!changed.automatic);
    assert!(seen[0].0 < changed.revision);
    assert!(changed.revision < seen[1].0);
    assert!(!seen[1].1);
    assert!(
        !app.state::<Updates>()
            .inner
            .lock()
            .unwrap()
            .status
            .automatic
    );
}
