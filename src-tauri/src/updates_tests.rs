//! Exercise official updater transport, signing and replacement on disposable data.
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    os::unix::process::ExitStatusExt,
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
use tauri_plugin_updater::UpdaterExt;

pub(crate) fn serve(responses: Vec<Vec<u8>>) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/latest.json", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let thread = std::thread::spawn(move || {
        for response in responses {
            let start = std::time::Instant::now();
            let mut stream = loop {
                if let Ok((stream, _)) = listener.accept() {
                    break stream;
                }
                assert!(
                    start.elapsed() < Duration::from_secs(10),
                    "Updater did not request the expected resource"
                );
                std::thread::sleep(Duration::from_millis(5));
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = [0; 4096];
            let count = stream.read(&mut request).unwrap();
            assert!(count > 0, "Expected an HTTP request before responding");
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                response.len()
            )
            .unwrap();
            stream.write_all(&response).unwrap();
        }
    });
    (url, thread)
}
pub(crate) fn cli(root: &Path, args: &[&str]) {
    let status = std::process::Command::new("node")
        .arg(root.join("node_modules/@tauri-apps/cli/tauri.js"))
        .args(args)
        .env("CI", "true")
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "Tauri test signing command failed; private output withheld"
    );
}
#[tokio::test]
async fn official_updater_checks_downloads_rejects_tampering_and_retries() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("test.key");
    let payload = dir.path().join("synthetic.bin");
    std::fs::write(&payload, b"synthetic update archive bytes").unwrap();
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
    let mut context = tauri::test::mock_context(tauri::test::noop_assets());
    context.config_mut().plugins.0.insert(
        "updater".into(),
        serde_json::json!({
            "pubkey":public.trim(),"dangerousInsecureTransportProtocol":true
        }),
    );
    let app = tauri::test::mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .build(context)
        .unwrap();
    let updater = |url: &str| {
        app.updater_builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .executable_path(dir.path().join("Memivy.app/Contents/MacOS/Memivy"))
            .endpoints(vec![url.parse().unwrap()])
            .unwrap()
            .build()
            .unwrap()
    };
    for tampered in [false, true, false] {
        // Separate servers keep requests and retries observable without a live release.
        let bytes = if tampered {
            b"tampered archive".to_vec()
        } else {
            std::fs::read(&payload).unwrap()
        };
        let (download_url, download_server) = serve(vec![bytes.clone()]);
        let manifest = serde_json::json!({"version":"99.0.0","notes":"Synthetic update", "platforms":{
            "darwin-aarch64":{"url":download_url,"signature":signature.trim()}
        }});
        let (url, check_server) = serve(vec![serde_json::to_vec(&manifest).unwrap()]);
        let update = updater(&url).check().await.unwrap().unwrap();
        assert_eq!(update.version, "99.0.0");
        let mut received = 0;
        let result = update.download(|n, _| received += n, || {}).await;
        assert_eq!(received, bytes.len());
        if tampered {
            assert!(result.is_err());
        } else {
            assert_eq!(result.unwrap(), bytes);
        }
        check_server.join().unwrap();
        download_server.join().unwrap();
    }
    let (url, server) = serve(vec![
        serde_json::to_vec(&serde_json::json!({"version":"0.0.0","platforms":{
            "darwin-aarch64":{"url":"https://example.com/unused","signature":signature.trim()}
        }}))
        .unwrap(),
    ]);
    assert!(updater(&url).check().await.unwrap().is_none());
    server.join().unwrap();
    let (url, server) = serve(vec![b"invalid manifest".to_vec()]);
    assert!(updater(&url).check().await.is_err());
    server.join().unwrap();
    // The listener is now gone: transport failure must not appear as 'up to date'.
    assert!(updater(&url).check().await.is_err());
}

#[tokio::test]
#[ignore = "Requires signed release archives and the legacy 0.1.9 MCP helper"]
async fn signed_release_archive_replaces_a_disposable_bundle() {
    let archive = std::path::PathBuf::from(
        std::env::var_os("MEMIVY_TEST_UPDATE_ARCHIVE").expect("Set MEMIVY_TEST_UPDATE_ARCHIVE"),
    );
    let legacy_archive =
        std::env::var_os("MEMIVY_TEST_LEGACY_UPDATE_ARCHIVE").map(std::path::PathBuf::from);
    let legacy_version = "0.1.9";
    let replacement_version = if legacy_archive.is_some() {
        env!("CARGO_PKG_VERSION")
    } else {
        // Keep the original single-archive invocation for historical 0.1.9 tests.
        legacy_version
    };
    let bytes = std::fs::read(&archive).unwrap();
    let signature = std::fs::read_to_string(format!("{}.sig", archive.display())).unwrap();
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.release.conf.json")).unwrap();
    let public = config["plugins"]["updater"]["pubkey"].as_str().unwrap();
    let mut context = tauri::test::mock_context(tauri::test::noop_assets());
    context.config_mut().plugins.0.insert(
        "updater".into(),
        serde_json::json!({
            "pubkey":public,"dangerousInsecureTransportProtocol":true
        }),
    );
    let app = tauri::test::mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .build(context)
        .unwrap();
    let (download_url, download_server) = serve(vec![bytes.clone()]);
    let (url,server)=serve(vec![serde_json::to_vec(&serde_json::json!({
        "version":"99.0.0","platforms":{"darwin-aarch64":{"url":download_url,"signature":signature.trim()}}
    })).unwrap()]);
    let scratch = tempfile::tempdir().unwrap();
    let installed = scratch.path().join("Memivy.app");
    let executable = installed.join("Contents/MacOS/memivy");
    std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
    std::fs::write(&executable, b"Old synthetic executable").unwrap();
    let sidecar = installed.join("Contents/MacOS/memivy-mcp");
    assert!(
        Command::new("/usr/bin/tar")
            .args(["-xzf"])
            .arg(legacy_archive.as_deref().unwrap_or(&archive))
            .arg("-C")
            .arg(scratch.path())
            .arg("Memivy.app/Contents/MacOS/memivy-mcp")
            .status()
            .unwrap()
            .success()
    );
    let old_marker = installed.join("old-bundle-marker");
    std::fs::write(&old_marker, b"Old synthetic bundle").unwrap();
    let library = scratch.path().join("library");
    let store = memivy_core::memory::MemoryStore::open_application(&library).unwrap();
    store.set_mcp_enabled(true).unwrap();
    let preference = std::fs::read(library.join("mcp.json")).unwrap();
    let updater = app
        .updater_builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .executable_path(&executable)
        .endpoints(vec![url.parse().unwrap()])
        .unwrap()
        .build()
        .unwrap();
    let update = updater.check().await.unwrap().unwrap();
    let verified = update.download(|_, _| {}, || {}).await.unwrap();
    assert_eq!(verified, bytes);
    server.join().unwrap();
    download_server.join().unwrap();
    // An unrecognized holder must never be signaled. A failed close releases
    // admission and leaves the downloaded package and original bundle intact.
    {
        let guard = store.prepare_app_update().unwrap();
        let session = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(guard.session_lock_path())
            .unwrap();
        session.lock_shared().unwrap();
        assert_eq!(
            crate::updates::close_mcp_sessions(&guard, &executable)
                .unwrap_err()
                .code,
            "update_mcp_busy",
        );
    }
    assert!(old_marker.is_file());

    let mut connected = McpChild::open(&sidecar, &library, legacy_version);
    let capture = connected.call(
        "tools/call",
        json!({
            "name": "memory_capture",
            "arguments": {
                "request_id": "55ff4a2a-80ac-4c9b-95b0-8d1d49bcf3b8",
                "text": "An apple orchard for updater verification.",
                "source_app": "Synthetic stdio client",
            },
        }),
    );
    let memory_id = capture["structuredContent"]["memory_id"].as_str().unwrap();
    // An identical executable serving another library must stay connected.
    let mut unrelated = McpChild::open(&sidecar, &scratch.path().join("unrelated"), legacy_version);
    let guard = store.prepare_app_update().unwrap();
    let sessions = crate::updates::close_mcp_sessions(&guard, &executable).unwrap();
    assert_eq!(connected.wait_for_exit().signal(), Some(15));
    update.install(&verified).unwrap();
    assert!(!old_marker.exists());
    assert!(executable.is_file());
    assert!(sidecar.is_file());
    assert_eq!(
        unrelated.call("tools/list", json!({}))["tools"]
            .as_array()
            .unwrap()
            .len(),
        2,
    );
    assert_eq!(std::fs::read(library.join("mcp.json")).unwrap(), preference);
    drop((guard, sessions));

    let mut reconnected = McpChild::open(&sidecar, &library, replacement_version);
    let search = reconnected.call(
        "tools/call",
        json!({
            "name": "memory_search",
            "arguments": {
                "queries": [{"text": "apple orchard", "keywords": ["apple", "orchard"]}]
            },
        }),
    );
    assert_eq!(
        search["structuredContent"]["items"][0]["record"]["id"],
        memory_id,
    );
    if legacy_archive.is_some() {
        let guard = store.prepare_app_update().unwrap();
        let sessions = crate::updates::close_mcp_sessions(&guard, &executable).unwrap();
        assert!(reconnected.wait_for_exit().success());
        drop((guard, sessions));
    }
    store.check_integrity().unwrap();
}

struct McpChild {
    child: Child,
    replies: mpsc::Receiver<Value>,
    request_id: u64,
}

impl McpChild {
    fn open(binary: &Path, library: &Path, expected_version: &str) -> Self {
        let mut child = Command::new(binary)
            .env("MEMIVY_DATA_DIR", library)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(serde_json::from_str(&line).unwrap()).is_err() {
                    break;
                }
            }
        });
        let mut session = Self {
            child,
            replies: rx,
            request_id: 0,
        };
        let info = session.call(
            "initialize",
            json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "synthetic-update-client", "version": "1"},
            }),
        );
        assert_eq!(info["serverInfo"]["name"], "memivy");
        assert_eq!(
            info["serverInfo"]["version"], expected_version,
            "Expected a v{expected_version} sidecar. Check MEMIVY_TEST_UPDATE_ARCHIVE and MEMIVY_TEST_LEGACY_UPDATE_ARCHIVE."
        );
        writeln!(
            session.child.stdin.as_mut().unwrap(),
            "{}",
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        )
        .unwrap();
        session
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        self.request_id += 1;
        writeln!(
            self.child.stdin.as_mut().unwrap(),
            "{}",
            json!({"jsonrpc": "2.0", "id": self.request_id, "method": method, "params": params}),
        )
        .unwrap();
        let reply = self.replies.recv_timeout(Duration::from_secs(8)).unwrap();
        assert_eq!(reply["id"], self.request_id);
        assert!(reply.get("error").is_none(), "{reply}");
        reply["result"].clone()
    }

    fn wait_for_exit(&mut self) -> ExitStatus {
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(start.elapsed() < Duration::from_secs(3));
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for McpChild {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
