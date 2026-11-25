use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use cpal::{FromSample, Sample};
use dkaudio::InputSink;

pub const RATE: u32 = 48_000;
pub const LIMIT: Duration = Duration::from_secs(10);

struct Pcm {
    samples: Vec<f32>,
    overflow: bool,
}

pub struct Recorder {
    pcm: Arc<Mutex<Pcm>>,
    xruns: Arc<AtomicUsize>,
    failure: Arc<Mutex<Option<String>>>,
}

impl Recorder {
    pub fn new() -> Self {
        Self {
            pcm: Arc::new(Mutex::new(Pcm {
                samples: Vec::with_capacity(RATE as usize * 2 * (LIMIT.as_secs() as usize + 1)),
                overflow: false,
            })),
            xruns: Arc::new(AtomicUsize::new(0)),
            failure: Arc::new(Mutex::new(None)),
        }
    }

    pub fn sink(&self, ready: mpsc::SyncSender<()>) -> impl InputSink {
        Sink {
            pcm: self.pcm.clone(),
            ready: Some(ready),
        }
    }

    pub fn errors(&self) -> impl FnMut(cpal::Error) + Send + 'static {
        let xruns = self.xruns.clone();
        let failure = self.failure.clone();
        move |error| match error.kind() {
            cpal::ErrorKind::Xrun => {
                xruns.fetch_add(1, Ordering::Relaxed);
            }
            cpal::ErrorKind::RealtimeDenied => {}
            _ => {
                *failure.lock().unwrap() = Some(error.to_string());
            }
        }
    }

    /// Call after pausing and dropping the stream. No disk I/O in the audio callback.
    pub fn finish(self, expected: Duration, file: &Path) -> Result<usize, String> {
        if let Some(error) = self.failure.lock().unwrap().take() {
            return Err(format!("Audio input stopped: {error}"));
        }
        let pcm = self.pcm.lock().unwrap();
        let frames = pcm.samples.len() / 2;
        let xruns = self.xruns.load(Ordering::Relaxed);
        if pcm.overflow || incomplete(frames, expected, xruns) {
            return Err(format!(
                "Incomplete USB capture: {frames} frames, {:.3} s elapsed, {xruns} overruns/underruns.",
                expected.as_secs_f64(),
            ));
        }
        write_pcm(file, &pcm.samples).map_err(|e| format!("Write PCM: {e}"))?;
        Ok(frames)
    }
}

struct Sink {
    pcm: Arc<Mutex<Pcm>>,
    ready: Option<mpsc::SyncSender<()>>,
}

impl InputSink for Sink {
    fn write<T: Sample>(&mut self, input: &[T], channels: usize)
    where
        f32: FromSample<T>,
    {
        let Ok(mut pcm) = self.pcm.try_lock() else {
            return;
        };
        let samples = input.len() / channels * 2;
        if samples == 0 {
            return;
        }
        if samples > pcm.samples.capacity() - pcm.samples.len() {
            pcm.overflow = true;
            return;
        }
        for frame in input.chunks_exact(channels) {
            pcm.samples
                .extend([f32::from_sample(frame[0]), f32::from_sample(frame[1])]);
        }
        if let Some(ready) = self.ready.take() {
            let _ = ready.try_send(());
        }
    }
}

fn incomplete(frames: usize, expected: Duration, xruns: usize) -> bool {
    let seconds = frames as f64 / f64::from(RATE);
    let expected = expected.as_secs_f64();
    xruns != 0 || seconds < 1.0 || (seconds - expected).abs() > (expected * 0.01).max(0.1)
}

fn write_pcm(file: &Path, samples: &[f32]) -> std::io::Result<()> {
    let mut out = BufWriter::new(OpenOptions::new().write(true).create_new(true).open(file)?);
    for sample in samples {
        out.write_all(&sample.to_le_bytes())?;
    }
    out.flush()
}

#[cfg(test)]
mod tests;
