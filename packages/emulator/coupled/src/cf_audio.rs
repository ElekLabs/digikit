//! Audio of a ColdFire-only emulation: a device whose MAIN mixes the voices of its peer CPU itself (`[emulator.peer_audio]`) hands the
//! emulator's PCM to the same output path the DSP session uses (`pcm_play::PcmPlayer`, then the cpal ring of `live-audio`). There is no DSP
//! worker: the PCM comes out of `Emulator::take_pcm` after each step.

use std::time::Instant;

use elektron_native_boot::pcm_play::{Mode, PcmPlayer, Recovery, SOURCE_RATE};
use serde_json::{Value, json};

/// Refill threshold after an underrun (the output ring is about one second): playback resumes once this much is queued.
const REFILL_SECONDS: f64 = 0.5;
/// Queued audio at which free running waits. It must lie above the refill threshold: after an underrun the player stays silent until
/// `REFILL_SECONDS` are queued, and a controller that stopped stepping below that would never produce them (the workbench stood still).
const PACE_SECONDS: f64 = REFILL_SECONDS + 0.1;

pub struct CfAudio {
    player: Option<PcmPlayer>,
    device_error: Option<String>,
    playback_requested: bool,
    /// Interleaved samples handed over so far.
    samples: u64,
    started: Instant,
}

impl CfAudio {
    /// With `playback` the default output device is opened; failing to open it is recorded and the session goes on without a sink.
    pub fn new(playback: bool) -> Self {
        let (player, device_error) = if playback {
            match PcmPlayer::spawn_with_recovery(Mode::Live, Recovery::Rebuffer(REFILL_SECONDS)) {
                Ok(p) => (Some(p), None),
                Err(e) => (None, Some(e)),
            }
        } else {
            (None, None)
        };
        Self {
            player,
            device_error,
            playback_requested: playback,
            samples: 0,
            started: Instant::now(),
        }
    }

    pub fn feed(&mut self, pcm: &[f32]) {
        if pcm.is_empty() {
            return;
        }
        self.samples += pcm.len() as u64;
        if let Some(player) = &self.player {
            player.push(pcm);
        }
    }

    /// True when the queued audio has reached the pace target.
    pub fn queue_ahead(&self) -> bool {
        self.player.as_ref().is_some_and(|p| {
            let q = p.queue_state();
            let queued = q.queued_device_frames as f64 / f64::from(q.device_rate.max(1))
                + q.feeder_pending_source_frames as f64 / f64::from(SOURCE_RATE);
            queued >= PACE_SECONDS
        })
    }

    pub fn report(&self) -> Value {
        let sink = self.player.as_ref().map(PcmPlayer::stats);
        let connected = sink.as_ref().is_some_and(|s| s.error.is_none());
        let audio = self.samples as f64 / 2.0 / f64::from(SOURCE_RATE);
        let wall = self.started.elapsed().as_secs_f64();
        json!({
            "schema_version": 1, "peer": "coldfire", "sample_rate": SOURCE_RATE,
            "output_device_sample_rate": self.player.as_ref().map(|p| p.device_rate),
            "frames": self.samples / 2 / 32, "source_pcm_frames": self.samples / 2,
            "source_audio_seconds": audio, "session_wall_seconds": wall,
            "audio_seconds_per_session_wall_second": if wall > 0.0 { audio / wall } else { 0.0 },
            "missing_blocks": 0, "halted": null, "flow_started": self.samples > 0,
            "playback_requested": self.playback_requested, "sink_connected": connected,
            "device_error": self.device_error,
            "output_device": self.player.as_ref().map(|p| p.device.clone()),
            "sink": sink, "dsp_tiers": null, "dsp_core": null,
        })
    }

    pub fn sink_underruns(&self) -> Option<u64> {
        self.player
            .as_ref()
            .map(|p| p.queue_state().underrun_events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_without_playback_counts_what_it_is_fed_and_reports_no_sink() {
        let mut audio = CfAudio::new(false);
        assert!(!audio.queue_ahead());
        audio.feed(&[]);
        audio.feed(&[0.5; 64]);
        let report = audio.report();
        assert_eq!(report["source_pcm_frames"], 32);
        assert_eq!(report["frames"], 1);
        assert_eq!(report["sink_connected"], false);
        assert!(report["output_device"].is_null());
        assert_eq!(report["playback_requested"], false);
    }
}
