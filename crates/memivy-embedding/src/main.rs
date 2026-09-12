//! Private single-purpose encoder. No database access or public network listener.
use llama_cpp_2::{
    context::params::{LlamaContextParams, LlamaPoolingType},
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{AddBos, LlamaModel, params::LlamaModelParams},
    token::LlamaToken,
};
use memivy_core::embedding::{
    self as emb,
    client::{Request, Response},
    *,
};
use std::{
    collections::VecDeque,
    fs,
    io::{BufRead, BufReader, Read, Write},
    num::NonZeroU32,
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    time::{Duration, Instant},
};
fn main() {
    if let Err(error) = run() {
        // Keep the bounded startup reason available to Settings, even before a socket exists.
        if let Some(root) = std::env::args_os().nth(2)
            && let Ok(dir) = socket_dir(&PathBuf::from(root))
        {
            let _ = write_json(&dir.join("startup-error.json"), &error);
        }
        eprintln!("{error}");
        std::process::exit(1);
    }
}
fn run() -> emb::Result<()> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--data-root")) {
        return Err("missing data root".into());
    }
    let root = PathBuf::from(args.next().ok_or("missing root")?);
    if !Preferences::read(&root)?.wanted() {
        return Ok(());
    }
    let dir = socket_dir(&root)?;
    let _guard = lock(&dir, "worker.lock")?;
    let path = cache::HfModelCache::for_user()?.verify()?;
    let mut backend = LlamaBackend::init().map_err(|_| "embedding_load_failed")?;
    backend.void_logs();
    if !backend.supports_gpu_offload() {
        return Err("embedding_metal_unavailable".into());
    }
    let model = LlamaModel::load_from_file(
        &backend,
        path,
        &LlamaModelParams::default().with_n_gpu_layers(u32::MAX),
    )
    .map_err(|_| "embedding_load_failed")?;
    if model.n_embd() != DIMENSIONS as i32 {
        return Err("embedding_response_invalid".into());
    }
    let params = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(8192))
        .with_n_batch(8192)
        .with_n_ubatch(8192)
        .with_embeddings(true)
        .with_pooling_type(LlamaPoolingType::Last);
    let mut context = model
        .new_context(&backend, params)
        .map_err(|_| "embedding_load_failed")?;
    encode(
        &model,
        &mut context,
        "Memivy 中文 multilingual verification",
    )?;
    let socket = dir.join("worker.sock");
    if socket.exists() {
        fs::remove_file(&socket).map_err(|_| "embedding_connection_lost")?;
    }
    let listener = UnixListener::bind(&socket).map_err(|_| "embedding_connection_lost")?;
    listener
        .set_nonblocking(true)
        .map_err(|_| "embedding_connection_lost")?;
    let mut high = VecDeque::new();
    let mut low = VecDeque::new();
    let mut last = Instant::now();
    loop {
        if !Preferences::read(&root).is_ok_and(|p| p.wanted())
            || last.elapsed() > Duration::from_secs(300)
        {
            break;
        }
        while let Ok((stream, _)) = listener.accept() {
            stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
            let mut line = String::new();
            if BufReader::new(&stream)
                .take(16 * 1024)
                .read_line(&mut line)
                .is_err()
                || line.is_empty()
            {
                continue;
            }
            let Ok(req) = serde_json::from_str::<Request>(&line) else {
                continue;
            };
            if high.len() + low.len() >= 32 {
                reply(stream, &req, Err("embedding_busy".into()));
                continue;
            }
            if req.kind == "query" {
                high.push_back((stream, req));
            } else {
                low.push_back((stream, req));
            }
        }
        let Some((stream, req)) = high.pop_front().or_else(|| low.pop_front()) else {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        };
        last = Instant::now();
        let result = (|| -> emb::Result<Vec<f32>> {
            if req.protocol != 1
                || req.fingerprint != fingerprint()
                || !matches!(req.kind.as_str(), "query" | "document")
                || req.text.is_empty()
                || req.text.chars().count() > MAX_CHARS
                || req.deadline_ms <= client::millis()
            {
                return Err("embedding_configuration_changed".into());
            }
            let vector = encode(&model, &mut context, &req.text)?;
            if req.deadline_ms <= client::millis() || !Preferences::read(&root)?.wanted() {
                return Err("embedding_timeout".into());
            }
            Ok(vector)
        })();
        reply(stream, &req, result);
    }
    fs::remove_file(socket).ok();
    Ok(())
}
fn reply(mut stream: UnixStream, req: &Request, result: emb::Result<Vec<f32>>) {
    stream
        .set_write_timeout(Some(Duration::from_millis(200)))
        .ok();
    let (vector, error) = match result {
        Ok(v) => (Some(v), None),
        Err(e) => (None, Some(e)),
    };
    let response = Response {
        id: req.id.clone(),
        fingerprint: fingerprint(),
        vector,
        error,
    };
    if serde_json::to_writer(&mut stream, &response).is_ok() {
        stream.write_all(b"\n").ok();
    }
}

fn encode(
    model: &LlamaModel,
    context: &mut llama_cpp_2::context::LlamaContext<'_>,
    text: &str,
) -> emb::Result<Vec<f32>> {
    if text.contains('\0') {
        return Err("embedding_input_invalid".into());
    }
    let mut tokens = model
        .str_to_token(text, AddBos::Never)
        .map_err(|_| "embedding_input_invalid")?;
    tokens.push(LlamaToken::new(151643));
    context.clear_kv_cache();
    let mut batch = LlamaBatch::new(8192, 1);
    for (i, token) in tokens.iter().enumerate() {
        batch
            .add(*token, i as i32, &[0], i + 1 == tokens.len())
            .map_err(|_| "embedding_input_invalid")?;
    }
    context
        .decode(&mut batch)
        .map_err(|_| "embedding_response_invalid")?;
    let mut vector = context
        .embeddings_seq_ith(0)
        .map_err(|_| "embedding_response_invalid")?
        .to_vec();
    let norm = vector
        .iter()
        .map(|v| (*v as f64).powi(2))
        .sum::<f64>()
        .sqrt();
    if !norm.is_finite() || norm == 0.0 {
        return Err("embedding_response_invalid".into());
    }
    for v in &mut vector {
        *v = (*v as f64 / norm) as f32;
    }
    vector_bytes(&vector)?;
    Ok(vector)
}
