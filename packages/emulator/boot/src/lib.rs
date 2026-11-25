//! Portable bounded Oracle boot diagnostic runtime.

mod capture;
pub mod card_file;
#[cfg(feature = "cf-mix")]
pub mod cfmix;
#[cfg(feature = "cf-profile")]
pub mod cfprof;
mod common;
pub mod cputime;
mod flash_hle;
mod fs_check;
mod guard;
mod late_send;
pub mod midi;
mod nor_records;
pub mod panel_leds;
#[cfg(feature = "play")]
pub mod pcm_play;
mod pcprobe;
mod peer_service;
mod ram_clear;
mod runtime;
mod softfloat;
mod spans;
pub mod spikelog;
mod telemetry;
mod watch;
pub mod wav;

pub use capture::Dspi2Capture;
pub use cputime::thread_cpu_ns;
pub use flash_hle::FlashWrites;
pub use guard::{GUARD_HIT_LIMIT, GuardHit, GuardRange, GuardReport, parse_guard_spec};
pub use pcprobe::{DEFAULT_KEEP, PcProbeSpec, ProbeSpec, parse_spec as parse_pc_probe_spec};
pub use periph::clock;
pub use telemetry::DiagnosticReport;
pub use watch::{After, FlushHit, WatchConfig, Writer, parse_ranges};

pub use runtime::{
    CAPTURE_MAX, CallResult, CallSpec, Capture, Emulator, GuestCall, PeerReport, REG_LOG_MAX,
    RegHit, Snapshot, Status,
};
pub use softfloat::ExecutionPolicy;
