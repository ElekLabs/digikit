//! The audio output path of a coupled session: one [`AudioOut`] takes the PCM of
//! whichever core produces it (the SHARC session today, a peer ColdFire's
//! `Emulator::take_pcm()` later) and does the player push, the tap, the capture
//! and the pacing query the controller uses. It knows no DSP types.
use crate::pcm_observer::{PcmConnection, PcmSource, PcmWriter};
use elektron_native_boot::wav::WavWriter;
use serde_json::{Map, Value, json};
use std::{
    fs::File,
    io::{self, Write},
    path::PathBuf,
    sync::Arc,
    time::Instant,
};

/// The free-running pace target without prefill (`--audio-buffer 0`).
pub const PACE_MIN_SECONDS: f64 = 0.05;

/// The sample rate of the PCM an [`AudioOut`] takes (the DSP's audio rate).
pub const SOURCE_RATE: u32 = 48_000;

/// The sink's queue as its feeder last published it.
#[derive(Clone, Copy, Debug)]
pub struct SinkQueue {
    pub queued_device_frames: u64,
    pub feeder_pending_source_frames: u64,
    pub device_rate: u32,
    pub underrun_events: u64,
    pub rebuffering: bool,
}

impl SinkQueue {
    fn to_json(self) -> Value {
        json!({
            "queued_device_frames": self.queued_device_frames,
            "feeder_pending_source_frames": self.feeder_pending_source_frames,
            "device_rate": self.device_rate,
            "underrun_events": self.underrun_events,
            "rebuffering": self.rebuffering,
        })
    }
}

/// A snapshot of the sink's statistics for the report.
pub struct SinkStats {
    /// The statistics as the report shows them (`sink`).
    pub json: Value,
    /// The sink has no error.
    pub connected: bool,
}

/// The output device behind [`AudioOut`]: `PcmPlayer` with feature `play`, a fake in tests.
pub trait PcmSink {
    fn push(&self, pcm: &[f32]);
    fn queue_state(&self) -> SinkQueue;
    fn stats(&self) -> SinkStats;
    fn device(&self) -> String;
    fn device_rate(&self) -> u32;
}

/// The cpal player of `elektron-native-boot` as a sink.
#[cfg(feature = "play")]
mod player {
    use super::{PcmSink, SOURCE_RATE, SinkQueue, SinkStats};
    use elektron_native_boot::pcm_play::{Mode, PcmPlayer, Recovery};
    use serde_json::json;

    const _: () = assert!(elektron_native_boot::pcm_play::SOURCE_RATE == SOURCE_RATE);

    impl PcmSink for PcmPlayer {
        fn push(&self, pcm: &[f32]) {
            PcmPlayer::push(self, pcm);
        }
        fn queue_state(&self) -> SinkQueue {
            let q = PcmPlayer::queue_state(self);
            SinkQueue {
                queued_device_frames: q.queued_device_frames,
                feeder_pending_source_frames: q.feeder_pending_source_frames,
                device_rate: q.device_rate,
                underrun_events: q.underrun_events,
                rebuffering: q.rebuffering,
            }
        }
        fn stats(&self) -> SinkStats {
            let stats = PcmPlayer::stats(self);
            SinkStats {
                connected: stats.error.is_none(),
                json: json!(stats),
            }
        }
        fn device(&self) -> String {
            self.device.clone()
        }
        fn device_rate(&self) -> u32 {
            self.device_rate
        }
    }

    /// The output player mode and underrun policy for a buffer length and audio mode.
    pub fn player_settings(buffer_seconds: f64, audio_mode: crate::AudioMode) -> (Mode, Recovery) {
        let mode = if buffer_seconds == 0.0 {
            Mode::Live
        } else {
            Mode::Buffer(buffer_seconds)
        };
        // The refill threshold must fit the ~1 s output ring.
        let refill = buffer_seconds.clamp(0.2, 0.9);
        let recovery = match audio_mode {
            crate::AudioMode::Rebuffer => Recovery::Rebuffer(refill),
            crate::AudioMode::Stretch => Recovery::Stretch(refill),
            crate::AudioMode::Gap => Recovery::Gap,
        };
        (mode, recovery)
    }
}
#[cfg(feature = "play")]
pub use player::player_settings;

/// The queued audio at which free running waits: the prefill, at least `PACE_MIN_SECONDS`.
pub fn pace_seconds(buffer_seconds: f64) -> f64 {
    buffer_seconds.max(PACE_MIN_SECONDS)
}

/// Output of one session: player, tap, capture, counters and the pace target.
pub struct AudioOut {
    pcm_windows: Option<(PcmWriter, PcmConnection)>,
    sink: Option<Box<dyn PcmSink>>,
    device_error: Option<String>,
    playback_requested: bool,
    /// Interleaved samples pushed so far.
    samples: u64,
    started: Instant,
    pcm_capture: Option<PcmCapture>,
    pcm_tap: Option<PcmTap>,
    /// Queued audio at which free running waits.
    pace_seconds: f64,
}

impl AudioOut {
    /// Without `playback` no device is opened. A failed `open` is recorded as the device error and
    /// the output goes on without a sink. `capture` runs after `open` and gives
    /// the capture to start with, if any (the caller decides where it comes from).
    pub fn new(
        playback: bool,
        pace_seconds: f64,
        open: impl FnOnce() -> Result<Box<dyn PcmSink>, String>,
        capture: impl FnOnce() -> Option<PcmCapture>,
    ) -> Self {
        let (sink, device_error) = if playback {
            match open() {
                Ok(sink) => (Some(sink), None),
                Err(error) => (None, Some(error)),
            }
        } else {
            (None, None)
        };
        Self {
            pcm_windows: None,
            sink,
            device_error,
            playback_requested: playback,
            samples: 0,
            started: Instant::now(),
            pcm_capture: capture(),
            pcm_tap: None,
            pace_seconds,
        }
    }

    /// Hand PCM to the capture, the sink and the tap.
    pub fn push(&mut self, pcm: &[f32]) {
        self.samples += pcm.len() as u64;
        if let Some(capture) = &mut self.pcm_capture {
            capture.write(pcm);
        }
        if let Some((writer, _)) = &mut self.pcm_windows {
            writer.push(pcm);
        }
        if let Some(sink) = &self.sink {
            sink.push(pcm);
        }
        if let Some(tap) = &mut self.pcm_tap {
            tap.feed(pcm);
        }
    }

    /// Interleaved samples pushed so far.
    pub fn samples(&self) -> u64 {
        self.samples
    }
    /// Audio seconds pushed (stereo, at the source rate).
    pub fn audio_seconds(&self) -> f64 {
        self.samples as f64 / 2.0 / SOURCE_RATE as f64
    }
    pub fn wall_seconds(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }
    pub fn pace_seconds(&self) -> f64 {
        self.pace_seconds
    }

    /// True when the device ring plus the feeder's pending input holds at least `pace_seconds`.
    /// False without a sink.
    pub fn queue_ahead(&self) -> bool {
        self.sink.as_ref().is_some_and(|sink| {
            let q = sink.queue_state();
            // Pending frames are at the 48 kHz source rate (pcm_play::SOURCE_RATE).
            let queued = q.queued_device_frames as f64 / f64::from(q.device_rate.max(1))
                + q.feeder_pending_source_frames as f64 / 48_000.0;
            queued >= self.pace_seconds
        })
    }
    /// The queue as the feeder last published it, with the pace target (for the spike log).
    pub fn queue_json(&self) -> Value {
        self.sink.as_ref().map_or(Value::Null, |sink| {
            let mut queue = sink.queue_state().to_json();
            queue["pace_seconds"] = self.pace_seconds.into();
            queue
        })
    }
    /// Underrun events of the sink since the session began (`None` without a sink).
    pub fn sink_underruns(&self) -> Option<u64> {
        self.sink
            .as_ref()
            .map(|sink| sink.queue_state().underrun_events)
    }

    pub(crate) fn observe_windows(&mut self, source: &Arc<PcmSource>) {
        self.pcm_windows = Some(source.connect(SOURCE_RATE));
    }

    #[cfg(feature = "sharc")]
    pub(crate) fn observes_windows(&self) -> bool {
        self.pcm_windows.is_some()
    }

    pub(crate) fn reset_windows(&mut self) {
        if let Some((writer, _)) = &mut self.pcm_windows {
            writer.reset();
        }
    }

    /// Replace the tap; `keep` retains every sample.
    pub fn observe(&mut self, keep: bool) {
        self.pcm_tap = Some(PcmTap::new(keep));
    }
    pub fn tap(&self) -> Option<&PcmTap> {
        self.pcm_tap.as_ref()
    }
    /// Replace the PCM capture with one for `path` (no length limit).
    pub fn start_capture(&mut self, path: &std::path::Path) -> Result<(), String> {
        let capture = PcmCapture::open(path, u64::MAX);
        if let Some(error) = capture.error {
            return Err(format!("{}: {error}", path.display()));
        }
        self.pcm_capture = Some(capture);
        Ok(())
    }
    /// Close the PCM capture, if any.
    pub fn stop_capture(&mut self) {
        self.pcm_capture = None;
    }

    /// The report fields about the output: sink, device, counters and capture.
    pub fn report_fields(&self) -> Map<String, Value> {
        let sink = self.sink.as_ref().map(|sink| sink.stats());
        let connected = sink.as_ref().is_some_and(|s| s.connected);
        let wall = self.wall_seconds();
        let audio = self.audio_seconds();
        let Value::Object(mut fields) = json!({
            "output_device_sample_rate": self.sink.as_ref().map(|sink| sink.device_rate()),
            "source_pcm_frames": self.samples / 2,
            "source_audio_seconds": audio, "session_wall_seconds": wall,
            "audio_seconds_per_session_wall_second": if wall > 0.0 { audio / wall } else { 0.0 },
            "flow_started": self.samples > 0,
            "playback_requested": self.playback_requested, "sink_connected": connected,
            "device_error": self.device_error,
            "output_device": self.sink.as_ref().map(|sink| sink.device()),
            "sink": sink.map(|s| s.json),
        }) else {
            unreachable!()
        };
        if let Some(capture) = &self.pcm_capture {
            fields.insert("pcm_capture_path".into(), capture.path.clone().into());
            fields.insert("pcm_capture_count".into(), capture.count.into());
            fields.insert("pcm_capture_error".into(), capture.error.clone().into());
        }
        fields
    }
}

/// Running statistics of the PCM the DSP produced, for headless hosts.
pub struct PcmTap {
    /// Interleaved f32 values seen.
    pub count: u64,
    pub nonzero: u64,
    pub non_finite: u64,
    pub peak: f32,
    pub sum_sq: f64,
    /// 100 ms windows whose RMS is above 1e-3 (a coarse activity measure).
    pub active_windows: u64,
    pub windows: u64,
    /// RMS of each 100 ms window, in order (at most `TAP_WINDOW_LIMIT`).
    pub window_rms: Vec<f32>,
    win_n: u64,
    win_sq: f64,
    hasher: sha2::Sha256,
    /// Every sample, when asked for.
    pub kept: Option<Vec<f32>>,
}

/// Interleaved values in 100 ms of stereo audio.
const TAP_WINDOW: u64 = SOURCE_RATE as u64 / 10 * 2;
const TAP_WINDOW_LIMIT: usize = 36_000;

impl PcmTap {
    pub fn new(keep: bool) -> Self {
        use sha2::Digest;
        Self {
            count: 0,
            nonzero: 0,
            non_finite: 0,
            peak: 0.0,
            sum_sq: 0.0,
            active_windows: 0,
            windows: 0,
            window_rms: Vec::new(),
            win_n: 0,
            win_sq: 0.0,
            hasher: sha2::Sha256::new(),
            kept: keep.then(Vec::new),
        }
    }
    pub(crate) fn feed(&mut self, pcm: &[f32]) {
        use sha2::Digest;
        let mut bytes = Vec::with_capacity(std::mem::size_of_val(pcm));
        for &v in pcm {
            bytes.extend_from_slice(&v.to_le_bytes());
            self.count += 1;
            if !v.is_finite() {
                self.non_finite += 1;
                continue;
            }
            self.nonzero += (v != 0.0) as u64;
            self.peak = self.peak.max(v.abs());
            let sq = f64::from(v) * f64::from(v);
            self.sum_sq += sq;
            self.win_sq += sq;
            self.win_n += 1;
            if self.win_n == TAP_WINDOW {
                self.windows += 1;
                let rms = (self.win_sq / TAP_WINDOW as f64).sqrt();
                self.active_windows += (rms > 1e-3) as u64;
                if self.window_rms.len() < TAP_WINDOW_LIMIT {
                    self.window_rms.push(rms as f32);
                }
                self.win_n = 0;
                self.win_sq = 0.0;
            }
        }
        self.hasher.update(&bytes);
        if let Some(kept) = &mut self.kept {
            kept.extend_from_slice(pcm);
        }
    }
    /// SHA-256 of the little-endian f32 bytes seen so far.
    pub fn sha256_hex(&self) -> String {
        use sha2::Digest;
        format!("{:x}", self.hasher.clone().finalize())
    }
    pub fn rms(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            (self.sum_sq / self.count as f64).sqrt()
        }
    }
    /// Audio seconds covered (stereo).
    pub fn seconds(&self) -> f64 {
        self.count as f64 / 2.0 / SOURCE_RATE as f64
    }
}

enum CaptureOut {
    Raw(File),
    Wav(WavWriter),
}

pub struct PcmCapture {
    out: Option<CaptureOut>,
    path: String,
    count: u64,
    /// Most interleaved samples written.
    limit: u64,
    error: Option<String>,
}

impl PcmCapture {
    /// The capture named by `DIGI_EMU_PCM_F32LE`, if set.
    pub fn from_env() -> Option<Self> {
        let path = std::env::var_os("DIGI_EMU_PCM_F32LE")?;
        // Unset means unlimited.
        let limit = std::env::var("DIGI_EMU_PCM_SECONDS")
            .ok()
            .and_then(|text| text.parse::<f64>().ok())
            .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
            .map_or(u64::MAX, |seconds| {
                (seconds * SOURCE_RATE as f64 * 2.0) as u64
            });
        Some(Self::open(&PathBuf::from(path), limit))
    }

    /// A `.wav` path gets a float WAV, anything else raw f32le. A failed
    /// open is recorded in `error` and nothing is written.
    fn open(path: &std::path::Path, limit: u64) -> Self {
        let wav = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"));
        let opened = if wav {
            WavWriter::create(path, SOURCE_RATE, 2).map(CaptureOut::Wav)
        } else {
            File::create(path).map(CaptureOut::Raw)
        };
        let (out, error) = match opened {
            Ok(out) => (Some(out), None),
            Err(error) => (None, Some(format!("PCM capture open: {error}"))),
        };
        Self {
            out,
            path: path.display().to_string(),
            count: 0,
            limit,
            error,
        }
    }

    fn write(&mut self, pcm: &[f32]) {
        let Some(out) = &mut self.out else {
            return;
        };
        let result = match out {
            CaptureOut::Raw(file) => write_pcm_f32le(file, &mut self.count, pcm, self.limit),
            CaptureOut::Wav(wav) => {
                let samples = pcm
                    .len()
                    .min(self.limit.saturating_sub(self.count) as usize);
                if samples == 0 {
                    Ok(())
                } else {
                    let result = wav.write(&pcm[..samples]);
                    if result.is_ok() {
                        self.count += samples as u64;
                    }
                    result
                }
            }
        };
        if let Err(error) = result {
            self.out = None;
            self.error = Some(format!("PCM capture write: {error}"));
        }
    }
}

fn write_pcm_f32le(
    writer: &mut impl Write,
    count: &mut u64,
    pcm: &[f32],
    limit: u64,
) -> io::Result<()> {
    let samples = pcm.len().min(limit.saturating_sub(*count) as usize);
    if samples == 0 {
        return Ok(());
    }
    let mut bytes = Vec::with_capacity(samples * size_of::<f32>());
    for sample in &pcm[..samples] {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    writer.write_all(&bytes)?;
    *count += samples as u64;
    Ok(())
}

#[cfg(test)]
mod tests;
