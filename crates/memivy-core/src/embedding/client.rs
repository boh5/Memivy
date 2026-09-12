//! One bounded Unix-socket client used by the app and MCP.
use super::*;
use std::{
    io::{BufRead, BufReader, Read},
    os::unix::net::UnixStream,
    process::{Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub protocol: u8,
    pub id: String,
    pub fingerprint: String,
    pub kind: String,
    pub text: String,
    pub deadline_ms: u64,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub id: String,
    pub fingerprint: String,
    pub vector: Option<Vec<f32>>,
    pub error: Option<String>,
}
pub fn millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn ready(root: &Path) -> bool {
    socket_dir(root)
        .ok()
        .is_some_and(|d| UnixStream::connect(d.join("worker.sock")).is_ok())
}
pub fn warmup(root: &Path) -> Result<()> {
    if ready(root) {
        return Ok(());
    }
    let root = root.to_owned();
    let dir = socket_dir(&root)?;
    let guard = match lock(&dir, "launch.lock") {
        Ok(g) => g,
        Err(_) => return Ok(()),
    };
    if !Preferences::read(&root)?.wanted() {
        return Err("embedding_disabled".into());
    }
    let exe = std::env::current_exe().map_err(io)?;
    let binary = exe
        .parent()
        .ok_or("embedding_component_missing")?
        .join("memivy-embedding");
    if !binary.is_file() {
        return Err("embedding_component_missing".into());
    }
    let error_path = dir.join("startup-error.json");
    if error_path.exists() {
        fs::remove_file(&error_path).map_err(io)?;
    }
    let mut child = Command::new(binary)
        .arg("--data-root")
        .arg(&root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(io)?;
    std::thread::spawn(move || {
        let began = std::time::Instant::now();
        while began.elapsed() < Duration::from_secs(60) {
            if ready(&root) {
                drop(guard);
                let _ = child.wait();
                return;
            }
            if let Ok(Some(status)) = child.try_wait() {
                if !status.success() && !error_path.exists() {
                    let _ = write_json(&error_path, &"embedding_component_missing");
                }
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = child.kill();
        let _ = child.wait();
    });
    Ok(())
}
pub fn encode(root: &Path, kind: &str, text: &str, timeout: Duration) -> Result<Vec<f32>> {
    if !Preferences::read(root)?.wanted() {
        return Err("embedding_disabled".into());
    }
    if !matches!(kind, "query" | "document") || text.chars().count() > MAX_CHARS || text.is_empty()
    {
        return Err("embedding_input_invalid".into());
    }
    let dir = socket_dir(root)?;
    let mut stream = match UnixStream::connect(dir.join("worker.sock")) {
        Ok(s) => s,
        Err(_) => {
            warmup(root)?;
            return Err("embedding_warming".into());
        }
    };
    stream.set_read_timeout(Some(timeout)).map_err(io)?;
    stream.set_write_timeout(Some(timeout)).map_err(io)?;
    let id = uuid::Uuid::new_v4().to_string();
    let req = Request {
        protocol: 1,
        id: id.clone(),
        fingerprint: fingerprint(),
        kind: kind.into(),
        text: text.into(),
        deadline_ms: millis() + timeout.as_millis() as u64,
    };
    serde_json::to_writer(&mut stream, &req)
        .map_err(|_| "embedding_connection_lost".to_string())?;
    stream
        .write_all(b"\n")
        .map_err(|_| "embedding_connection_lost".to_string())?;
    let mut line = String::new();
    BufReader::new(stream)
        .take(32 * 1024)
        .read_line(&mut line)
        .map_err(|_| "embedding_timeout".to_string())?;
    let response: Response =
        serde_json::from_str(&line).map_err(|_| "embedding_response_invalid".to_string())?;
    if response.id != id || response.fingerprint != fingerprint() {
        return Err("embedding_configuration_changed".into());
    }
    let vector = response
        .vector
        .ok_or_else(|| process_error(response.error.as_deref()).to_string())?;
    vector_bytes(&vector)?;
    Ok(vector)
}

pub fn startup_error(root: &Path) -> Option<String> {
    let p = socket_dir(root).ok()?.join("startup-error.json");
    let bytes = fs::read(p).ok()?;
    if bytes.len() > 4096 {
        return None;
    }
    serde_json::from_slice::<String>(&bytes)
        .ok()
        .map(|error| process_error(Some(&error)).to_string())
}

fn process_error(error: Option<&str>) -> &'static str {
    match error {
        Some("model_cache_io") => "model_cache_io",
        Some("model_cache_path") => "model_cache_path",
        Some("model_cache_mismatch") => "model_cache_mismatch",
        Some("model_download_incomplete") => "model_download_incomplete",
        Some("embedding_settings_invalid") => "embedding_settings_invalid",
        Some("embedding_connection_lost") => "embedding_connection_lost",
        Some("embedding_disabled") => "embedding_disabled",
        Some("embedding_busy") => "embedding_busy",
        Some("embedding_input_invalid") => "embedding_input_invalid",
        Some("embedding_configuration_changed") => "embedding_configuration_changed",
        Some("embedding_timeout") => "embedding_timeout",
        Some("embedding_metal_unavailable") => "embedding_metal_unavailable",
        Some("embedding_load_failed") => "embedding_load_failed",
        Some("embedding_component_missing") => "embedding_component_missing",
        _ => "embedding_response_invalid",
    }
}
#[cfg(test)]
mod error_tests {
    #[test]
    fn process_errors_use_only_known_codes() {
        for code in [
            "model_cache_io",
            "model_cache_path",
            "model_cache_mismatch",
            "model_download_incomplete",
            "embedding_connection_lost",
            "embedding_metal_unavailable",
        ] {
            assert_eq!(super::process_error(Some(code)), code);
        }
        assert_eq!(
            super::process_error(Some("embedding_busy")),
            "embedding_busy"
        );
        assert_eq!(
            super::process_error(Some("private payload or old translated text")),
            "embedding_response_invalid"
        );
    }
}
