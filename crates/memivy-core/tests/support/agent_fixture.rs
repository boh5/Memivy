use memivy_core::model::ModelConfig;
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub struct FixtureServer {
    thread: Option<std::thread::JoinHandle<()>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
}
impl FixtureServer {
    pub fn join(mut self) -> std::thread::Result<()> {
        self.thread.take().unwrap().join()
    }
}
impl Drop for FixtureServer {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
pub type Requests = Arc<Mutex<Vec<Value>>>;
pub struct Response {
    pub status: u16,
    pub parts: Vec<String>,
}
impl Response {
    pub fn stream(body: String) -> Self {
        Self {
            status: 200,
            parts: vec![body],
        }
    }
}
pub fn fixture(
    count: usize,
    mut respond: impl FnMut(usize, &Value) -> Response + Send + 'static,
) -> (ModelConfig, Requests, FixtureServer) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let config = ModelConfig {
        provider: Default::default(),
        base_url: format!("http://{}/v1", listener.local_addr().unwrap()),
        model: "synthetic-agent".into(),
        api_key: None,
        max_output_tokens: None,
        output_token_parameter: Default::default(),
        disable_reasoning: false,
    };
    let requests = Arc::new(Mutex::new(vec![]));
    let log = requests.clone();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stopped = stop.clone();
    let server = std::thread::spawn(move || {
        for index in 0..count {
            let start = Instant::now();
            let mut socket = loop {
                if stopped.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            start.elapsed() < Duration::from_secs(15),
                            "expected model request {index}"
                        );
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut header = vec![];
            while !header.ends_with(b"\r\n\r\n") {
                let mut b = [0];
                socket.read_exact(&mut b).unwrap();
                header.push(b[0]);
            }
            let size: usize = String::from_utf8(header)
                .unwrap()
                .lines()
                .find_map(|line| {
                    line.to_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse().unwrap())
                })
                .unwrap();
            let mut body = vec![0; size];
            socket.read_exact(&mut body).unwrap();
            let request: Value = serde_json::from_slice(&body).unwrap();
            log.lock().unwrap().push(request.clone());
            let response = respond(index, &request);
            let length: usize = response.parts.iter().map(String::len).sum();
            let content_type = if request["stream"] == true {
                "text/event-stream"
            } else {
                "application/json"
            };
            if write!(socket,"HTTP/1.1 {} Synthetic\r\nContent-Type: {content_type}\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n",response.status).is_err(){continue;}
            for part in response.parts {
                if socket.write_all(part.as_bytes()).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    });
    (
        config,
        requests,
        FixtureServer {
            thread: Some(server),
            stop,
        },
    )
}

pub fn sse_delta(delta: Value, finish: Value) -> String {
    format!(
        "data: {}\n\n",
        json!({"choices":[{"index":0,"delta":delta,"finish_reason":finish}]})
    )
}
pub fn sse_text(text: &str) -> String {
    sse_delta(json!({"role":"assistant","content":text}), json!("stop")) + "data: [DONE]\n\n"
}
pub fn sse_tool(call_id: &str, name: &str, args: Value) -> String {
    sse_delta(
        json!({"role":"assistant","reasoning_content":"synthetic reasoning retained for protocol","tool_calls":[{"index":0,"id":call_id,"type":"function","function":{"name":name,"arguments":args.to_string()}}]}),
        json!("tool_calls"),
    ) + "data: [DONE]\n\n"
}
