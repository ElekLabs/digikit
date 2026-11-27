use super::install::Install;
use super::stats::Stats;
use core::ffi::c_void;

/// `DK_VH_MAGIC`: 'DKVH'.
pub const MAGIC: u32 = 0x444b_5648;
/// The major version this crate speaks. A host of another major is refused.
pub const MAJOR: u16 = 1;
/// The newest minor this crate knows. A host with a lower minor is refused.
pub const MINOR: u16 = 0;
/// Range entries of an install (`DK_MAX_PARAMS`).
pub const RANGES: usize = 8;
/// `DK_VH_STEREO`: the host can play a stereo voice.
pub const FLAG_STEREO: u32 = 1;

pub(super) type InstallFn = unsafe extern "C" fn(*mut c_void, *const Install<'_>, *mut u32) -> i32;
pub(super) type BindFn = unsafe extern "C" fn(*mut c_void, u32, u32) -> i32;
pub(super) type RetireFn = unsafe extern "C" fn(*mut c_void, u32) -> i32;
pub(super) type StatsFn = unsafe extern "C" fn(*mut c_void, u32, *mut Stats) -> i32;

/// `dk_voice_host_v1`, as the host wrote it.
#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct Table {
    pub(super) magic: u32,
    pub(super) major: u16,
    pub(super) minor: u16,
    pub(super) size: u32,
    pub(super) tracks: u16,
    pub(super) types_max: u16,
    pub(super) payload_bytes_max: u32,
    pub(super) state_bytes_max: u32,
    pub(super) budget: u32,
    pub(super) flags: u32,
    pub(super) ctx: *mut c_void,
    pub(super) install: Option<InstallFn>,
    pub(super) bind: Option<BindFn>,
    pub(super) retire: Option<RetireFn>,
    pub(super) stats: Option<StatsFn>,
}
