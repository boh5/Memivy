use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::Duration,
};
pub struct Engine {
    child: Child,
    input: ChildStdin,
    output: Receiver<String>,
    pub backend: String,
}
impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Engine {
    pub fn load() -> Result<Self, String> {
        Self::spawn("metal").or_else(|_| Self::spawn("cpu"))
    }
    fn spawn(backend: &str) -> Result<Self, String> {
        let binary = std::env::current_exe()
            .map_err(|_| "app_path_unavailable")?
            .with_file_name("memivy-speech");
        let mut child = Command::new(binary)
            .arg(backend)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "voice_component_missing")?;
        let input = child.stdin.take().ok_or("voice_pipe_unavailable")?;
        let stdout = child.stdout.take().ok_or("voice_pipe_unavailable")?;
        let (tx, output) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    _ => {
                        if line.len() > 100_000 || tx.send(line).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        let mut engine = Self {
            child,
            input,
            output,
            backend: backend.into(),
        };
        let ready = engine.response(Duration::from_secs(90))?;
        if ready["event"] != "ready" || ready["sample_rate"] != 16000 {
            return Err("voice_initialization_failed".into());
        }
        engine.backend = ready["backend"].as_str().unwrap_or(backend).into();
        Ok(engine)
    }
    fn response(&mut self, timeout: Duration) -> Result<Value, String> {
        let line = self
            .output
            .recv_timeout(timeout)
            .map_err(|_| "voice_timeout")?;
        serde_json::from_str(&line).map_err(|_| "voice_response_invalid".into())
    }
    pub fn transcribe(&mut self, samples: &[f32]) -> Result<String, String> {
        // Buffer the bounded request: writing each JSON scalar to ChildStdin
        // separately would turn a short clip into hundreds of thousands of syscalls.
        let mut request = serde_json::to_vec(&json!({"id":"segment","samples":samples}))
            .map_err(|_| "voice_request_invalid")?;
        request.push(b'\n');
        self.input
            .write_all(&request)
            .and_then(|_| self.input.flush())
            .map_err(|_| "voice_connection_lost")?;
        let result = self.response(Duration::from_secs(90))?;
        if result["id"] != "segment" || result["eos"] != true {
            return Err("voice_transcription_failed".into());
        }
        result["text"]
            .as_str()
            .filter(|s| s.len() <= 16000)
            .map(str::to_owned)
            .ok_or("voice_response_invalid".into())
    }
}
