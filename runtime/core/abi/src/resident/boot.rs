//! `BootFns`: the boot record the resident core keeps (`boot.rs`): what the package reports as it loads, takes down and reloads modules.

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BootFns {
    /// The path of selection entry `k`, which the boot's load loop reads now.
    pub note_boot_path: unsafe extern "C" fn(k: u32, path: *const u8),
    /// A module's load ended: `k`, its path, the result, its missing needs (`NEEDS_LEN` bytes) and its name (`LOG_NAME` bytes).
    pub report_module: unsafe extern "C" fn(
        k: u32,
        path: *const u8,
        result: i32,
        missing: *const u8,
        name: *const u8,
    ),
    /// A provider file's load ended: its path and length, the result, its hash and its base.
    pub report_provider:
        unsafe extern "C" fn(path: *const u8, len: u32, result: i32, hash: u32, base: u32),
    pub report_dsp: unsafe extern "C" fn(idx: u32, result: i32),
    pub report_taken_down: unsafe extern "C" fn(idx: u32, code: i32),
    pub begin_live: unsafe extern "C" fn(k: u32),
    pub restore_live: unsafe extern "C" fn(k: u32),
    pub unloaded: unsafe extern "C" fn(k: u32),
    /// The index a live job loads the file at `path` into: 0 and the index in `*out`, or an `E_*` code; `live` is `SEL_MAX` flags.
    pub live_index: unsafe extern "C" fn(path: *const u8, live: *const bool, out: *mut u32) -> i32,
    /// The record of a finished live load: `k`, its path, the result, its missing needs and its name.
    pub account_load: unsafe extern "C" fn(
        k: u32,
        path: *const u8,
        result: i32,
        missing: *const u8,
        name: *const u8,
    ),
}
