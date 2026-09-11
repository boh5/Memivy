use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{LlamaModel, params::LlamaModelParams},
    mtmd::{MtmdBitmap, MtmdContext, MtmdContextParams, MtmdInputText},
    sampling::LlamaSampler,
};
use serde_json::json;
#[derive(serde::Deserialize)]
struct Request {
    id: String,
    samples: Vec<f32>,
}
use std::{
    io::{self, BufRead, Write},
    num::NonZeroU32,
    time::Instant,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let cache = memivy_core::speech::SpeechCache::for_user()?;
    cache.verify()?;
    let gpu = a.get(1).is_none_or(|s| s != "cpu");
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get().min(4)) as i32;
    let started = Instant::now();
    let mut backend = LlamaBackend::init()?;
    if gpu && !backend.supports_gpu_offload() {
        return Err("GPU offload unavailable".into());
    }
    backend.void_logs();
    let model = LlamaModel::load_from_file(
        &backend,
        cache.path(&memivy_core::speech::MODELS[0]),
        &LlamaModelParams::default().with_n_gpu_layers(if gpu { u32::MAX } else { 0 }),
    )?;
    let mtmd = MtmdContext::init_from_file(
        cache
            .path(&memivy_core::speech::MODELS[1])
            .to_str()
            .ok_or("invalid model path")?,
        &model,
        &MtmdContextParams {
            use_gpu: gpu,
            n_threads: threads,
            print_timings: false,
            ..Default::default()
        },
    )?;
    let cp = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(2048))
        .with_n_batch(1024)
        .with_n_ubatch(1024)
        .with_n_threads(threads)
        .with_n_threads_batch(threads)
        .with_offload_kqv(gpu)
        .with_op_offload(gpu);
    let mut ctx = model.new_context(&backend, cp)?;
    println!(
        "{}",
        json!({"event":"ready","load_ms":started.elapsed().as_secs_f64()*1000.0,"backend":if gpu {"Metal GPU"} else {"CPU"},"threads":threads,"sample_rate":mtmd.get_audio_sample_rate()})
    );
    io::stdout().flush()?;
    for line in io::stdin().lock().lines() {
        let req: Request = serde_json::from_str(&line?)?;
        let started = Instant::now();
        let pcm = req.samples;
        if pcm.is_empty() || pcm.iter().any(|v| !v.is_finite() || v.abs() > 1.01) {
            return Err("invalid PCM".into());
        }
        if pcm.len() > 16000 * 20 {
            return Err("audio exceeds bounded probe input".into());
        }
        ctx.clear_kv_cache();
        let bitmap = MtmdBitmap::from_audio_data(&pcm)?;
        let prompt = "<|im_start|>system\n<|im_end|>\n<|im_start|>user\n<__media__><|im_end|>\n<|im_start|>assistant\n";
        let chunks = mtmd.tokenize(
            MtmdInputText {
                text: prompt.into(),
                add_special: true,
                parse_special: true,
            },
            &[&bitmap],
        )?;
        let past = chunks.eval_chunks(&mtmd, &ctx, 0, 0, 1024, true)?;
        let prefill_ms = started.elapsed().as_secs_f64() * 1000.0;
        let mut sampler = LlamaSampler::greedy();
        let mut out = String::new();
        let mut n = 0;
        let mut stopped = false;
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        for past in (past..).take(1024) {
            if started.elapsed().as_secs() > 90 {
                break;
            }
            let token = sampler.sample(&ctx, -1);
            sampler.accept(token);
            if model.is_eog_token(token) {
                stopped = true;
                break;
            }
            out.push_str(&model.token_to_piece(token, &mut decoder, true, None)?);
            n += 1;
            let mut batch = LlamaBatch::new(1, 1);
            batch.add(token, past, &[0], true)?;
            ctx.decode(&mut batch)?;
        }
        if !stopped {
            return Err("transcription exceeded decoding boundary".into());
        }
        let text = memivy_core::speech::transcript(&out)?;
        println!(
            "{}",
            json!({"text":text,"id":req.id,"duration_s":pcm.len() as f64/16000.0,"elapsed_ms":started.elapsed().as_secs_f64()*1000.0,"prefill_ms":prefill_ms,"tokens":n,"eos":stopped,"raw":out})
        );
        io::stdout().flush()?;
    }
    Ok(())
}
