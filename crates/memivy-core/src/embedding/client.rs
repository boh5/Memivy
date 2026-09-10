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
        return Err("语义检索已关闭".into());
    }
    let exe = std::env::current_exe().map_err(io)?;
    let binary = exe
        .parent()
        .ok_or("找不到本地模型程序")?
        .join("memivy-embedding");
    if !binary.is_file() {
        return Err("本地模型程序缺失，请重新安装完整应用".into());
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
                if !status.success() {
                    let _ = write_json(&error_path, &"模型进程启动失败，请重试或重新安装完整应用");
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
        return Err("语义检索已关闭".into());
    }
    if !matches!(kind, "query" | "document") || text.chars().count() > MAX_CHARS || text.is_empty()
    {
        return Err("编码输入超出边界".into());
    }
    let dir = socket_dir(root)?;
    let mut stream = match UnixStream::connect(dir.join("worker.sock")) {
        Ok(s) => s,
        Err(_) => {
            warmup(root)?;
            return Err("本地模型正在预热，本次使用字面检索".into());
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
    serde_json::to_writer(&mut stream, &req).map_err(|_| "模型连接中断，请重试".to_string())?;
    stream
        .write_all(b"\n")
        .map_err(|_| "模型连接中断，请重试".to_string())?;
    let mut line = String::new();
    BufReader::new(stream)
        .take(32 * 1024)
        .read_line(&mut line)
        .map_err(|_| "模型编码超时，本次使用字面检索".to_string())?;
    let response: Response =
        serde_json::from_str(&line).map_err(|_| "模型进程响应无效".to_string())?;
    if response.id != id || response.fingerprint != fingerprint() {
        return Err("模型编码配置已变化".into());
    }
    let vector = response
        .vector
        .ok_or_else(|| response.error.unwrap_or("模型编码失败".into()))?;
    vector_bytes(&vector)?;
    Ok(vector)
}

pub fn startup_error(root: &Path) -> Option<String> {
    let p = socket_dir(root).ok()?.join("startup-error.json");
    let bytes = fs::read(p).ok()?;
    if bytes.len() > 4096 {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}
