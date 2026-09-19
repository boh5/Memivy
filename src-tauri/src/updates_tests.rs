//! Exercise the official updater transport and signature verification without installing an app.
use std::{
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    time::Duration,
};
use tauri_plugin_updater::UpdaterExt;

fn serve(responses: Vec<Vec<u8>>) -> (String, std::thread::JoinHandle<()>) {
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
fn cli(root: &Path, args: &[&str]) {
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
#[ignore = "Requires a locally built signed release archive"]
async fn built_release_archive_matches_the_shipped_public_key() {
    let archive = std::path::PathBuf::from(
        std::env::var_os("MEMIVY_TEST_UPDATE_ARCHIVE").expect("Set MEMIVY_TEST_UPDATE_ARCHIVE"),
    );
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
    let updater = app
        .updater_builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .executable_path(
            archive
                .parent()
                .unwrap()
                .join("Memivy.app/Contents/MacOS/Memivy"),
        )
        .endpoints(vec![url.parse().unwrap()])
        .unwrap()
        .build()
        .unwrap();
    let update = updater.check().await.unwrap().unwrap();
    assert_eq!(update.download(|_, _| {}, || {}).await.unwrap(), bytes);
    server.join().unwrap();
    download_server.join().unwrap();
}
