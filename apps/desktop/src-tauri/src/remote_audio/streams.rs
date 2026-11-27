//! Select and own a matching input and an independent system output.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::thread;

use cpal::Sample;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    Device, DeviceId, ErrorKind, FromSample, SampleFormat, SizedSample, Stream,
    SupportedStreamConfig,
};

use digi_coupled::pcm_observer::{PcmConnection, PcmSource};

use super::stereo::{self, Capture, Playback};
use dkaudio::{InputSink, exact_device, open_input, stereo_config};

#[cfg(windows)]
mod wasapi;

pub(super) struct Streams {
    // Drop playback first so a stopped monitor cannot play queued input.
    output: Stream,
    input: Stream,
    signal: Arc<StreamSignal>,
    pcm_connection: Option<PcmConnection>,
}

impl Streams {
    pub(super) fn start(
        input: &str,
        pcm_source: &Arc<PcmSource>,
    ) -> Result<(Self, String), String> {
        let host = cpal::default_host();
        let input_id = input
            .parse::<DeviceId>()
            .map_err(|error| format!("Invalid audio input ID: {error}"))?;
        let input = exact_device(&host, &input_id)
            .map_err(|error| format!("Selected audio input: {error}"))?;

        let output = host
            .default_output_device()
            .ok_or("No default audio output")?;
        let output_id = output
            .id()
            .map_err(|error| format!("Identify default audio output: {error}"))?;

        // The default CoreAudio and WASAPI handles can reroute when the
        // system output changes. Open the resolved endpoint instead.
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let output = exact_device(&host, &output_id)
            .map_err(|error| format!("Default audio output: {error}"))?;

        #[cfg(windows)]
        let input_description = input
            .description()
            .map_err(|error| format!("Describe audio input: {error}"))?;
        let output_description = output
            .description()
            .map_err(|error| format!("Describe audio output: {error}"))?;
        let same_device = input_id == output_id;
        #[cfg(windows)]
        let same_device = same_device || {
            // WASAPI input and output are different endpoints even when they
            // belong to the same USB interface or headset.
            wasapi::same_container(&input_id, &output_id).unwrap_or_else(|| {
                input_description
                    .driver()
                    .zip(output_description.driver())
                    .is_some_and(|(input, output)| input.eq_ignore_ascii_case(output))
            })
        };
        if same_device {
            return Err("Audio input and system output are on the same device. Choose a different system audio output before monitoring.".into());
        }

        let input_config = stereo_config(&input, true, None)
            .map_err(|error| format!("Configure audio input: {error}"))?;
        let output_config = stereo_config(&output, false, None)
            .map_err(|error| format!("Configure audio output: {error}"))?;

        #[cfg(target_os = "macos")]
        super::permission::check()?;

        let signal = Arc::new(StreamSignal {
            #[cfg(test)]
            rates: [input_config.sample_rate(), output_config.sample_rate()],
            ..StreamSignal::default()
        });
        let (mut capture, playback) =
            stereo::monitor(input_config.sample_rate(), output_config.sample_rate());
        let (writer, connection) = pcm_source.connect(input_config.sample_rate());
        capture.observe(writer);
        let input = open_input(
            &input,
            input_config,
            MonitorInput {
                capture,
                signal: signal.clone(),
            },
            on_error(&signal, false),
        )?;
        let output = playback_stream(output, output_config, playback, signal.clone())?;
        let streams = Self {
            output,
            input,
            signal,
            pcm_connection: Some(connection),
        };
        streams
            .input
            .play()
            .map_err(|error| format!("Start audio input: {error}"))?;
        streams
            .output
            .play()
            .map_err(|error| format!("Start audio output: {error}"))?;

        if let Some(error) = streams.error() {
            return Err(error);
        }
        Ok((streams, output_description.name().to_owned()))
    }

    // CoreAudio can still own an audio unit while delivering a device-change
    // notification. Silence its callbacks and pause both endpoints before Stop replies.
    pub(super) fn pause(mut self) -> Result<(), String> {
        self.stop()
    }

    fn stop(&mut self) -> Result<(), String> {
        self.pcm_connection = None;
        if self.signal.closing.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let output = self
            .output
            .pause()
            .map_err(|error| format!("Stop audio output: {error}"));
        let input = self
            .input
            .pause()
            .map_err(|error| format!("Stop audio input: {error}"));
        output.and(input)
    }

    #[cfg(test)]
    pub(super) fn signal(&self) -> Arc<StreamSignal> {
        self.signal.clone()
    }

    pub(super) fn error(&self) -> Option<String> {
        let code = self.signal.failure.load(Ordering::Acquire);
        if code == 0 {
            return None;
        }
        let direction = if code & 0x80 == 0 { "input" } else { "output" };
        let reason = match code & 0x7f {
            1 => "device disconnected",
            2 => "access was denied",
            3 => "device or configuration changed",
            4 => "audio subsystem stopped",
            5 => "device is busy",
            _ => "stream failed",
        };
        Some(format!("Audio {direction} stopped: {reason}."))
    }
}

impl Drop for Streams {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[derive(Default)]
pub(super) struct StreamSignal {
    closing: AtomicBool,
    failure: AtomicU8,
    #[cfg(test)]
    captured: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    played: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    peaks: [std::sync::atomic::AtomicU32; 2],
    #[cfg(test)]
    rates: [u32; 2],
}

fn on_error(signal: &Arc<StreamSignal>, output: bool) -> impl FnMut(cpal::Error) + use<> {
    // CoreAudio unregisters device listeners asynchronously. A late notification
    // belongs to the old stream and must not keep its state alive after stop.
    let signal = Arc::downgrade(signal);
    let owner = thread::current();
    move |error| {
        let reason = match error.kind() {
            ErrorKind::RealtimeDenied | ErrorKind::Xrun => return,
            ErrorKind::DeviceNotAvailable => 1,
            ErrorKind::PermissionDenied => 2,
            ErrorKind::DeviceChanged | ErrorKind::StreamInvalidated => 3,
            ErrorKind::HostUnavailable => 4,
            ErrorKind::DeviceBusy => 5,
            _ => 6,
        };
        let Some(signal) = signal.upgrade() else {
            return;
        };
        let code = reason | if output { 0x80 } else { 0 };
        if signal
            .failure
            .compare_exchange(0, code, Ordering::Release, Ordering::Relaxed)
            .is_ok()
        {
            owner.unpark();
        }
    }
}

macro_rules! for_pcm {
    ($format:expr, $open:ident) => {
        match $format {
            SampleFormat::F32 => $open!(f32),
            SampleFormat::F64 => $open!(f64),
            SampleFormat::I8 => $open!(i8),
            SampleFormat::I16 => $open!(i16),
            SampleFormat::I24 => $open!(cpal::I24),
            SampleFormat::I32 => $open!(i32),
            SampleFormat::I64 => $open!(i64),
            SampleFormat::U8 => $open!(u8),
            SampleFormat::U16 => $open!(u16),
            SampleFormat::U24 => $open!(cpal::U24),
            SampleFormat::U32 => $open!(u32),
            SampleFormat::U64 => $open!(u64),
            format => return Err(format!("Unsupported audio sample format: {format}")),
        }
    };
}

struct MonitorInput {
    capture: Capture,
    signal: Arc<StreamSignal>,
}

impl InputSink for MonitorInput {
    fn write<T: Sample>(&mut self, data: &[T], channels: usize)
    where
        f32: FromSample<T>,
    {
        if !self.signal.closing.load(Ordering::Acquire)
            && self.signal.failure.load(Ordering::Acquire) == 0
        {
            self.capture.write(data, channels);
            #[cfg(test)]
            self.signal
                .captured
                .fetch_add(data.len() / channels, Ordering::Relaxed);
        }
    }
}

fn playback_stream(
    device: Device,
    supported: SupportedStreamConfig,
    playback: Playback,
    signal: Arc<StreamSignal>,
) -> Result<Stream, String> {
    let channels = supported.channels() as usize;
    let config = supported.config();
    let callback_error = on_error(&signal, true);
    macro_rules! open {
        ($sample:ty) => {
            device.build_output_stream(
                config,
                playback_callback::<$sample>(playback, channels, signal),
                callback_error,
                None,
            )
        };
    }
    for_pcm!(supported.sample_format(), open).map_err(|error| format!("Open audio output: {error}"))
}

fn playback_callback<T: SizedSample + FromSample<f32>>(
    mut playback: Playback,
    channels: usize,
    signal: Arc<StreamSignal>,
) -> impl FnMut(&mut [T], &cpal::OutputCallbackInfo)
where
    f32: FromSample<T>,
{
    move |data, _| {
        if !signal.closing.load(Ordering::Acquire) && signal.failure.load(Ordering::Acquire) == 0 {
            let _frames = playback.fill(data, channels);
            #[cfg(test)]
            {
                let mut peaks = [0.0_f32; 2];
                for frame in data.chunks_exact(channels) {
                    for ch in 0..2 {
                        peaks[ch] = peaks[ch].max(f32::from_sample(frame[ch]).abs());
                    }
                }
                for (counter, peak) in signal.peaks.iter().zip(peaks) {
                    counter.fetch_max(peak.to_bits(), Ordering::Relaxed);
                }
                signal.played.fetch_add(_frames, Ordering::Relaxed);
            }
        } else {
            data.fill(T::EQUILIBRIUM);
        }
    }
}

#[cfg(test)]
mod tests;
