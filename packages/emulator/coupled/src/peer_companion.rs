//! The companion of a device whose second processor is a ColdFire (`[emulator] peer = "coldfire"`, the Syntakt). The
//! peer CPU runs in lock step inside `Emulator::step_chunk`, so there is nothing to wait for: its audio, if the
//! device's MAIN mixes it (`[emulator.peer_audio]`), is collected from `Emulator::take_pcm` after each step.
use crate::{
    audio_out::{AudioOut, PcmCapture, SOURCE_RATE, pace_seconds},
    companion::{Companion, Kind, ResponderCompanion},
};
use elektron_native_boot::Emulator;
use serde_json::{Value, json};
use std::any::Any;

/// Queued audio at which free running waits. It must lie above the player's refill threshold: after an
/// underrun the player stays silent until the threshold is queued, and a controller that stopped stepping below
/// it would never produce that audio.
const PACE_SECONDS: f64 = REFILL_SECONDS + 0.1;
/// Refill threshold after an underrun (the output ring is about one second).
const REFILL_SECONDS: f64 = 0.5;

/// Stereo frames per audio frame of the peer's mixer (32 per 0.667 ms).
const FRAMES_PER_BLOCK: u64 = 32;

pub struct PeerCompanion {
    /// The device mixes the peer's voices (`Emulator::has_peer_audio`).
    audio: bool,
    /// Interleaved samples collected so far.
    samples: u64,
}

impl PeerCompanion {
    pub fn new(audio: bool) -> Self {
        Self { audio, samples: 0 }
    }
}

impl Companion for PeerCompanion {
    fn kind(&self) -> Kind {
        Kind::Peer
    }
    fn makes_audio(&self) -> bool {
        self.audio
    }
    /// Lock step: the peer has run when the ColdFire step returns.
    fn sync(&mut self) {}
    fn poll(&mut self, emulator: &mut Emulator, out: &mut AudioOut) {
        if !self.audio {
            return;
        }
        let pcm = emulator.take_pcm();
        if !pcm.is_empty() {
            self.samples += pcm.len() as u64;
            out.push(&pcm);
        }
    }
    /// A stop of the peer shows as an `Emulator` error.
    fn halted(&self) -> Option<String> {
        None
    }
    fn export_state(&mut self, _: &mut Emulator) -> Result<(Vec<u8>, u64), String> {
        Err("peer CPU has no exported state".into())
    }
    fn report(&self, _: &Emulator) -> Value {
        json!({
            "schema_version": 1, "peer": "coldfire", "sample_rate": SOURCE_RATE,
            "frames": self.samples / 2 / FRAMES_PER_BLOCK, "missing_blocks": 0, "halted": null,
            "dsp_tiers": null, "dsp_core": null,
        })
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The companion of `emulator`'s second ColdFire and the output it needs, or `None` when the device has none.
/// With `audio`, a peer CPU that mixes the device's voices gets the player (`DIGI_EMU_PEER_AUDIO`); without it
/// (patched builds) the peer runs without audio and the output is silent.
pub(crate) fn for_emulator(
    emulator: &Emulator,
    audio: bool,
) -> Option<(Box<dyn Companion>, AudioOut)> {
    if emulator.has_peer_cpu() {
        let audio = audio && emulator.has_peer_audio();
        let out = if audio {
            audio_out_for_peer()
        } else {
            crate::silent_out()
        };
        Some((Box::new(PeerCompanion::new(audio)), out))
    } else if emulator.has_peer_responder() {
        Some((Box::new(ResponderCompanion), crate::silent_out()))
    } else {
        None
    }
}

/// `DIGI_EMU_PEER_AUDIO=1`: a device whose MAIN mixes its peer's voices plays them on the default output device
/// (the desktop launcher sets it; the daemon and the tests do not).
fn peer_audio_playback() -> bool {
    std::env::var_os("DIGI_EMU_PEER_AUDIO").is_some_and(|v| v != "0")
}

/// The output of a peer CPU's audio: a live player that rebuffers after an underrun, when asked for.
fn audio_out_for_peer() -> AudioOut {
    audio_out_with(peer_audio_playback(), PcmCapture::from_env)
}

fn audio_out_with(playback: bool, capture: impl FnOnce() -> Option<PcmCapture>) -> AudioOut {
    AudioOut::new(playback, pace_seconds(PACE_SECONDS), open_player, capture)
}

#[cfg(feature = "play")]
fn open_player() -> Result<Box<dyn crate::audio_out::PcmSink>, String> {
    use elektron_native_boot::pcm_play::{Mode, PcmPlayer, Recovery};
    PcmPlayer::spawn_with_recovery(Mode::Live, Recovery::Rebuffer(REFILL_SECONDS))
        .map(|p| Box::new(p) as Box<dyn crate::audio_out::PcmSink>)
}

#[cfg(not(feature = "play"))]
fn open_player() -> Result<Box<dyn crate::audio_out::PcmSink>, String> {
    Err("built without the play feature".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pace_target_is_above_the_refill_threshold() {
        assert!(pace_seconds(PACE_SECONDS) > REFILL_SECONDS);
    }

    #[test]
    fn audio_depends_on_the_device_not_on_the_kind() {
        assert!(PeerCompanion::new(true).makes_audio());
        assert!(!PeerCompanion::new(false).makes_audio());
        assert_eq!(PeerCompanion::new(false).kind(), Kind::Peer);
        assert!(Kind::Peer.makes_audio() && !Kind::Responder.makes_audio());
        assert!(!ResponderCompanion.makes_audio());
    }
}
