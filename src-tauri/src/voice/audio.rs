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
        .map_err(|_| "microphone_permission".into())
}
pub fn record(service: Arc<Service>, id: String, stop: Arc<AtomicBool>) -> Result<(), String> {
    if stop.load(Ordering::SeqCst) {
        return Ok(());
    }
    let device = cpal::default_host()
        .default_input_device()
        .ok_or("microphone_missing")?;
    let supported = device
        .default_input_config()
        .map_err(|_| "microphone_unavailable")?;
    if stop.load(Ordering::SeqCst) {
        return Ok(());
    }
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let rate = config.sample_rate.0 as usize;
    if !(8000..=192000).contains(&rate) || config.channels == 0 || config.channels > 32 {
        return Err("microphone_format".into());
    }
    let (tx, rx) = mpsc::sync_channel(128);
    let failed = Arc::new(AtomicBool::new(false));
    let stream = match format {
        cpal::SampleFormat::F32 => stream::<f32>(&device, &config, tx, failed.clone()),
        cpal::SampleFormat::I16 => stream::<i16>(&device, &config, tx, failed.clone()),
        cpal::SampleFormat::U16 => stream::<u16>(&device, &config, tx, failed.clone()),
        _ => Err("microphone_format".into()),
    }?;
    if stop.load(Ordering::SeqCst) {
        return Ok(());
    }
    let mut resampler =
        FftFixedIn::<f32>::new(rate, 16000, FRAME, 2, 1).map_err(|_| "audio_conversion_failed")?;
    let mut pending = Vec::with_capacity(FRAME * 4);
    let mut segment = Segmenter::default();
    stream.play().map_err(|_| "microphone_permission")?;
    if !service.listening(&id) {
        return Ok(());
    }
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
                    .map_err(|_| "audio_conversion_failed")?;
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
            .map_err(|_| "audio_conversion_failed")?;
        feed(&service, &id, &mut segment, &out[0])?;
    }
    let out = resampler
        .process_partial::<Vec<f32>>(None, None)
        .map_err(|_| "audio_conversion_failed")?;
    feed(&service, &id, &mut segment, &out[0])?;
    if let Some(pcm) = segment.finish() {
        service.enqueue(&id, pcm)?;
    }
    if interrupted {
        return Err("audio_interrupted".into());
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
        // Loudness decides pause boundaries, never whether quiet audio survives.
        // The hard segment limit bounds the buffer even before a loud frame.
        None
    }
    fn finish(&mut self) -> Option<Vec<f32>> {
        let mut samples = std::mem::take(&mut self.samples);
        self.speech = false;
        self.silent = 0;
        if !samples.iter().any(|sample| *sample != 0.0) {
            return None;
        }
        // Keep even a short final syllable; pad the ASR frame instead of dropping it.
        if samples.len() < 1600 {
            samples.resize(1600, 0.0);
        }
        Some(samples)
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
        assert!(s.samples.len() < 16000 * 12);
        assert!(s.finish().is_none());
    }
    #[test]
    fn quiet_audio_is_retained_in_full_for_transcription_and_retry() {
        let mut s = Segmenter::default();
        for _ in 0..300 {
            assert!(s.push(&[0.001; 320], 0.001).is_none());
        }
        assert_eq!(s.finish().unwrap(), vec![0.001; 96000]);
    }
    #[test]
    fn sub_frame_final_audio_is_retained_with_silence_padding() {
        let mut s = Segmenter::default();
        assert!(s.push(&[0.001; 800], 0.001).is_none());
        let tail = s.finish().unwrap();
        assert_eq!(&tail[..800], &[0.001; 800]);
        assert_eq!(&tail[800..], &[0.0; 800]);
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
