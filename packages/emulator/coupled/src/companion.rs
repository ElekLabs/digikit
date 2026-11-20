//! The companion processor of a coupled run: whatever answers the ColdFire's DSPI2 link and, if it makes sound,
//! hands the PCM to the runtime's [`AudioOut`]. That is the SHARC+ engine (`SharcCompanion`), a ColdFire peer
//! CPU (`PeerCompanion`), the zero answerer (`ZeroCompanion`), the mailbox responder (`ResponderCompanion`)
//! or nothing (`NoCompanion`).
//!
//! Scheduling stays outside the trait. The SHARC+ runs on a worker thread behind `periph::dspi::Peer` and
//! `sync()` is its barrier; the peer CPU runs in lock step inside the board, so its `sync()` does nothing.
use crate::audio_out::AudioOut;
use elektron_native_boot::Emulator;
use serde_json::Value;
use std::any::Any;

/// Which companion a runtime has. `as_str` is the name the reports use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Kind {
    /// The SHARC+ engine on its worker thread.
    Sharc,
    /// A second ColdFire that runs a program (`[emulator] peer = "coldfire"`), in lock step inside the board.
    Peer,
    /// The mailbox responder stands in for the second ColdFire: no program, no PCM.
    Responder,
    /// The DSP side answers zeros; no engine, no PCM.
    Zero,
    /// No DSP side.
    None,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sharc => "sharc",
            Self::Peer => "peer",
            Self::Responder => "responder",
            Self::Zero => "zero",
            Self::None => "none",
        }
    }
    /// Whether a companion of this kind can produce PCM. A peer CPU only does when the device mixes its voices
    /// (`Companion::makes_audio` says).
    pub fn makes_audio(self) -> bool {
        matches!(self, Self::Sharc | Self::Peer)
    }
}

pub trait Companion {
    fn kind(&self) -> Kind;
    /// Whether this companion produces PCM now (the runtime's `has_audio`).
    fn makes_audio(&self) -> bool {
        self.kind().makes_audio()
    }
    /// Wait until the companion has finished every frame sent so far. The PCM this completes is pushed by the
    /// next [`poll`](Self::poll).
    fn sync(&mut self);
    /// Collect what the companion has finished without waiting and push its PCM into `out`.
    fn poll(&mut self, emulator: &mut Emulator, out: &mut AudioOut);
    /// Called right before the runtime steps the ColdFire (timing marks for the link diagnostics).
    fn step_begin(&mut self) {}
    /// Why the companion stopped, once it has.
    fn halted(&self) -> Option<String>;
    /// The companion's canonical state and its instruction count, after every frame sent so far.
    fn export_state(&mut self, emulator: &mut Emulator) -> Result<(Vec<u8>, u64), String>;
    /// The companion's part of the health report (`Value::Null` when it has none).
    fn report(&self, emulator: &Emulator) -> Value;
    /// Called with the merged health report (companion part and output part) each time the runtime builds
    /// one, for diagnostics the companion prints itself.
    fn log_report(&self, _merged: &Value) {}
    /// For data only one kind has (the SHARC+ patch, peek, DDR monitor, profiles and counters).
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// The default DSPI2 peer answers zeros (`AudioOptions::zero`): nothing to run or collect.
pub struct ZeroCompanion;

/// The mailbox responder in place of a peer CPU (`use_peer_responder`): nothing to run or collect.
pub struct ResponderCompanion;

/// No companion: a ColdFire-only run.
pub struct NoCompanion;

macro_rules! inert {
    ($ty:ident, $kind:expr, $why:literal) => {
        impl Companion for $ty {
            fn kind(&self) -> Kind {
                $kind
            }
            fn sync(&mut self) {}
            fn poll(&mut self, _: &mut Emulator, _: &mut AudioOut) {}
            fn halted(&self) -> Option<String> {
                None
            }
            fn export_state(&mut self, _: &mut Emulator) -> Result<(Vec<u8>, u64), String> {
                Err($why.into())
            }
            fn report(&self, _: &Emulator) -> Value {
                Value::Null
            }
            fn as_any(&self) -> &dyn Any {
                self
            }
            fn as_any_mut(&mut self) -> &mut dyn Any {
                self
            }
        }
    };
}
inert!(ZeroCompanion, Kind::Zero, "zero-DSP mode has no DSP state");
inert!(
    ResponderCompanion,
    Kind::Responder,
    "the mailbox responder has no state"
);
inert!(NoCompanion, Kind::None, "no DSP companion");
