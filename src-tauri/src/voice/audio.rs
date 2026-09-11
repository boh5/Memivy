use super::Service;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rubato::{FftFixedIn, Resampler};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    time::Duration,
};
const FRAME: usize = 1024;
fn stream<T: cpal::SizedSample>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    tx: SyncSender<Vec<f32>>,
    failed: Arc<AtomicBool>,
) -> Result<cpal::Stream, String>
where
    f32: cpal::FromSample<T>,
{
    let channels = config.channels as usize;
    let dropped = failed.clone();
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                // Never wait for inference, disk IO, or the UI on CoreAudio's callback.
                for block in data.chunks(FRAME * channels) {
                    let mono = block
                        .chunks_exact(channels)
                        .map(|frame| {
                            frame
                                .iter()
                                .map(|s| <f32 as cpal::FromSample<T>>::from_sample_(*s))
                                .sum::<f32>()
                                / channels as f32
                        })
                        .collect();
                    if tx.try_send(mono).is_err() {
                        dropped.store(true, Ordering::Relaxed);
                    }
                }
            },
            move |_| {
                failed.store(true, Ordering::Relaxed);
            },
            None,
        )
        .map_err(|_| {
            "无法打开麦克风。请在系统设置 → 隐私与安全性 → 麦克风中允许 Memivy，并检查输入设备。"
                .into()
        })
}
pub fn record(service: Arc<Service>, id: String, stop: Arc<AtomicBool>) -> Result<(), String> {
    if stop.load(Ordering::SeqCst) {
        return Ok(());
    }
    let device = cpal::default_host()
        .default_input_device()
        .ok_or("未找到麦克风，请连接输入设备")?;
    let supported = device
        .default_input_config()
        .map_err(|_| "麦克风不可用，请检查系统输入设备")?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let rate = config.sample_rate.0 as usize;
    if !(8000..=192000).contains(&rate) || config.channels == 0 || config.channels > 32 {
        return Err("麦克风格式不受支持".into());
    }
    let (tx, rx) = mpsc::sync_channel(128);
    let failed = Arc::new(AtomicBool::new(false));
    let stream = match format {
        cpal::SampleFormat::F32 => stream::<f32>(&device, &config, tx, failed.clone()),
        cpal::SampleFormat::I16 => stream::<i16>(&device, &config, tx, failed.clone()),
        cpal::SampleFormat::U16 => stream::<u16>(&device, &config, tx, failed.clone()),
        _ => Err("麦克风采样格式不受支持".into()),
    }?;
    let mut resampler =
        FftFixedIn::<f32>::new(rate, 16000, FRAME, 2, 1).map_err(|_| "音频转换初始化失败")?;
    let mut pending = Vec::with_capacity(FRAME * 4);
    let mut segment = Segmenter::default();
    stream
        .play()
        .map_err(|_| "麦克风未能开始录音，请检查系统权限")?;
    service.listening(&id);
    let mut count = 0usize;
    let mut interrupted = false;
    while !stop.load(Ordering::SeqCst) && count < rate * 300 {
        if failed.load(Ordering::Relaxed) {
            interrupted = true;
            break;
        }
        if let Ok(samples) = rx.recv_timeout(Duration::from_millis(50)) {
            count += samples.len();
            pending.extend(samples);
            while pending.len() >= FRAME {
                let input: Vec<f32> = pending.drain(..FRAME).collect();
                let out = resampler
                    .process(&[input], None)
                    .map_err(|_| "音频转换失败")?;
                feed(&service, &id, &mut segment, &out[0])?;
            }
        }
    }
    drop(stream);
    for samples in rx.try_iter() {
        pending.extend(samples);
    }
    while !pending.is_empty() {
        let n = pending.len().min(FRAME);
        let input: Vec<f32> = pending.drain(..n).collect();
        let out = resampler
            .process_partial(Some(&[input]), None)
            .map_err(|_| "音频末尾转换失败")?;
        feed(&service, &id, &mut segment, &out[0])?;
    }
    let out = resampler
        .process_partial::<Vec<f32>>(None, None)
        .map_err(|_| "音频末尾转换失败")?;
    feed(&service, &id, &mut segment, &out[0])?;
    if let Some(pcm) = segment.finish() {
        service.enqueue(&id, pcm)?;
    }
    if interrupted {
        return Err("录音设备中断或处理不及，已保留收到的音频。可重试转写并核对是否缺字。".into());
    }
    Ok(())
}
fn feed(service: &Service, id: &str, segment: &mut Segmenter, pcm: &[f32]) -> Result<(), String> {
    for frame in pcm.chunks(320) {
        let level = (frame.iter().map(|v| v * v).sum::<f32>() / frame.len().max(1) as f32).sqrt();
        service.level(id, level, frame.len());
        if let Some(samples) = segment.push(frame, level) {
            service.enqueue(id, samples)?;
        }
    }
    Ok(())
}
#[derive(Default)]
struct Segmenter {
    samples: Vec<f32>,
    silent: usize,
    speech: bool,
}
impl Segmenter {
    fn push(&mut self, pcm: &[f32], level: f32) -> Option<Vec<f32>> {
        self.samples.extend_from_slice(pcm);
        if level > 0.003 {
            self.silent = 0;
            self.speech = true;
        } else {
            self.silent += pcm.len();
        }
        if self.samples.len() >= 16000 * 12 || (self.speech && self.silent >= 11200) {
            return self.finish();
        }
        // Keep a short pre-roll during silence, including quiet word onsets.
        if !self.speech && self.samples.len() > 8000 {
            let n = self.samples.len() - 8000;
            self.samples.drain(..n);
        }
        None
    }
    fn finish(&mut self) -> Option<Vec<f32>> {
        let samples = std::mem::take(&mut self.samples);
        let speech = std::mem::take(&mut self.speech);
        self.silent = 0;
        (speech && samples.len() >= 1600).then_some(samples)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn silence_is_bounded_and_never_transcribed() {
        let mut s = Segmenter::default();
        for _ in 0..1000 {
            assert!(s.push(&[0.; 320], 0.).is_none());
        }
        assert!(s.samples.len() <= 8000);
        assert!(s.finish().is_none());
    }
    #[test]
    fn pauses_and_hard_limits_preserve_all_speech() {
        let mut s = Segmenter::default();
        assert!(s.push(&[0.1; 3200], 0.1).is_none());
        let a = s.push(&[0.; 11200], 0.).unwrap();
        assert_eq!(a.len(), 14400);
        let a = s.push(&vec![0.1; 192000], 0.1).unwrap();
        assert_eq!(a.len(), 192000);
        assert!(s.finish().is_none());
    }
}
