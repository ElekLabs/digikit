//! Map native PCM to the first stereo pair and carry it between audio callbacks.

use cpal::{FromSample, Sample, SizedSample};
use digi_coupled::pcm_observer::PcmWriter;
use fixed_resample::audioadapter_buffers::direct::InterleavedSlice;
use fixed_resample::{
    Interleaved, PacketResampler, ReadStatus, ResampleQuality, ResamplerConfig,
    ResamplingChannelConfig, ResamplingCons, ResamplingProd, resampling_channel,
};

const CHANNELS: usize = 2;
const BLOCK_FRAMES: usize = 256;

pub(super) fn monitor(input_rate: u32, output_rate: u32) -> (Capture, Playback) {
    let resampler = (input_rate != output_rate).then(|| {
        PacketResampler::new(
            CHANNELS,
            input_rate,
            output_rate,
            ResamplerConfig {
                quality: ResampleQuality::HighWithLowLatency,
                chunk_size: BLOCK_FRAMES,
            },
        )
    });
    // Resample before enqueueing. fixed-resample 0.13.0's combined channel
    // overestimates free space when upsampling.
    // 100 ms covers a 4096-frame capture quantum at 44.1 kHz with room for a
    // resampler block. At sustained excess the consumer trims stale frames.
    let (prod, mut cons) = resampling_channel::<f32>(
        CHANNELS,
        output_rate,
        output_rate,
        true,
        ResamplingChannelConfig {
            latency_seconds: 0.10,
            capacity_seconds: 0.25,
            overflow_autocorrect_percent_threshold: Some(75.0),
            underflow_autocorrect_percent_threshold: None,
            ..ResamplingChannelConfig::default()
        },
    );
    // The output stream will read as soon as it starts. Accept capture that
    // arrives while CPAL is starting the two streams.
    cons.set_output_stream_ready(true);
    #[cfg(test)]
    let startup_frames =
        cons.latency_frames() + resampler.as_ref().map_or(0, PacketResampler::output_delay);
    (
        Capture {
            prod,
            resampler,
            pcm_writer: None,
        },
        Playback {
            #[cfg(test)]
            startup_frames,
            cons,
        },
    )
}

pub(super) struct Capture {
    prod: ResamplingProd<f32>,
    resampler: Option<PacketResampler<f32, Interleaved<f32>>>,
    pcm_writer: Option<PcmWriter>,
}

impl Capture {
    pub(super) fn observe(&mut self, writer: PcmWriter) {
        self.pcm_writer = Some(writer);
    }

    pub(super) fn write<T: Sample>(&mut self, input: &[T], channels: usize)
    where
        f32: FromSample<T>,
    {
        let mut stereo = [0.0; BLOCK_FRAMES * CHANNELS];
        for block in input.chunks(BLOCK_FRAMES * channels) {
            let frames = block.len() / channels;
            let stereo = &mut stereo[..frames * CHANNELS];
            for (pair, frame) in stereo
                .chunks_exact_mut(CHANNELS)
                .zip(block.chunks_exact(channels))
            {
                pair[0] = pcm(f32::from_sample(frame[0]));
                pair[1] = pcm(f32::from_sample(frame[1]));
            }

            if let Some(writer) = &mut self.pcm_writer {
                writer.push(stereo);
            }

            // Once the channel is full, its producer drops frames that do not
            // fit. Continue feeding the resampler to preserve its filter state.
            if let Some(resampler) = &mut self.resampler {
                let input = InterleavedSlice::new(stereo, CHANNELS, frames).unwrap();
                resampler.process(
                    &input,
                    None,
                    None,
                    |packet, _| {
                        self.prod.push_interleaved(packet);
                    },
                    None,
                    // Leave the filter delay intact. fixed-resample 0.13.0
                    // can panic while doing its optional in-place trimming.
                    false,
                );
            } else {
                self.prod.push_interleaved(stereo);
            }
        }
    }
}

pub(super) struct Playback {
    cons: ResamplingCons<f32>,
    #[cfg(test)]
    startup_frames: usize,
}

impl Playback {
    pub(super) fn fill<T: SizedSample + FromSample<f32>>(
        &mut self,
        output: &mut [T],
        channels: usize,
    ) -> usize {
        let ceiling = if T::FORMAT.is_float() {
            1.0
        } else {
            // CPAL's integer conversions require a sample strictly below +1.
            1.0_f32.next_down()
        };
        let mut stereo = [0.0; BLOCK_FRAMES * CHANNELS];
        let mut played = 0;
        output.fill(T::EQUILIBRIUM);
        for block in output.chunks_mut(BLOCK_FRAMES * channels) {
            let requested = block.len() / channels;
            let stereo = &mut stereo[..requested * CHANNELS];
            let (frames, _discarded) = match self.cons.read_interleaved(stereo, true) {
                ReadStatus::Ok => (requested, 0),
                ReadStatus::OverflowCorrected {
                    num_frames_discarded,
                } => (requested, num_frames_discarded),
                ReadStatus::UnderflowOccurred { num_frames_read } => (num_frames_read, 0),
                ReadStatus::InputNotReady => (0, 0),
            };
            for (frame, pair) in block
                .chunks_exact_mut(channels)
                .zip(stereo[..frames * CHANNELS].chunks_exact(CHANNELS))
            {
                frame[0] = T::from_sample(pcm(pair[0]).min(ceiling));
                frame[1] = T::from_sample(pcm(pair[1]).min(ceiling));
            }
            played += frames;
            #[cfg(test)]
            {
                played -= frames.min(self.startup_frames);
                self.startup_frames = self.startup_frames.saturating_sub(frames + _discarded);
            }
            if frames < requested {
                break;
            }
        }
        played
    }
}

fn pcm(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests;
