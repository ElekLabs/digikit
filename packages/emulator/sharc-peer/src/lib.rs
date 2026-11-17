//! The SHARC+ side of a coupled emulation. The ColdFire emulator (`elektron-native-boot`) knows only the
//! `periph::dspi::Peer` trait; this crate implements it with the native SHARC+ engine (`SharcPeer`,
//! `ThreadedPeer`) and holds what loads and patches the DSP image.

pub mod ddr_monitor;
pub mod dsp_patch;
pub mod dsp_stream;

#[cfg(feature = "sharc")]
mod peer;
#[cfg(feature = "sharc")]
pub use peer::*;

#[cfg(feature = "host")]
pub mod cold;
#[cfg(feature = "host")]
pub mod dsp_core;
