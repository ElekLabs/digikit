//! `PlatformFns`: the device and the firmware under the core, as the package uses them (the `PackagePlatform` methods).

use crate::storage::{DirEntry, StatWire};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PlatformFns {
    /// The firmware's heap: null when it is out of memory.
    pub alloc: unsafe extern "C" fn(bytes: u32) -> *mut u8,
    pub free: unsafe extern "C" fn(p: *mut u8),
    /// The DSP block counter: 0 and the value in `*out`, or negative when there is none.
    pub blocks: unsafe extern "C" fn(out: *mut u32) -> i32,
    pub irq_mask: extern "C" fn() -> u32,
    pub irq_restore: extern "C" fn(sr: u32),
    pub flush: extern "C" fn(start: u32, len: u32),
    pub info: unsafe extern "C" fn(name_hash: u32) -> *const u32,
    /// The device address of a `[[slot]]` word of the info table (0: none).
    pub info_slot: unsafe extern "C" fn(name_hash: u32) -> u32,
    /// 0 and the slot index in `*out`, or negative when the entry is no DSP slot.
    pub info_dsp_slot: unsafe extern "C" fn(name_hash: u32, out: *mut u32) -> i32,
    pub read_code: unsafe extern "C" fn(addr: u32, out: *mut u8, len: u32),
    pub write_code: unsafe extern "C" fn(addr: u32, bytes: *const u8, len: u32),
    pub write_word: unsafe extern "C" fn(addr: u32, value: u32),
    /// 0 and the transport state in `*out`, or negative when the platform cannot read it.
    pub play_state: unsafe extern "C" fn(out: *mut u32) -> i32,
    pub device_id: extern "C" fn() -> u32,
    pub sample_path_max: extern "C" fn() -> u32,
    pub has_dsp: extern "C" fn() -> bool,
    pub dsp_headroom: extern "C" fn() -> u32,
    pub dsp_voices: extern "C" fn() -> u32,
    pub dsp_block_cycles: extern "C" fn() -> u32,
    /// 0 and the `StatWire` in `*out`, or an `E_*` code.
    pub stat: unsafe extern "C" fn(root: u32, path: *const u8, len: u32, out: *mut StatWire) -> i32,
    /// `stat` and the first 64 bytes of the file in `head`: the count of head bytes (0 or 64), or an `E_*` code.
    pub stat_head: unsafe extern "C" fn(
        root: u32,
        path: *const u8,
        len: u32,
        out: *mut StatWire,
        head: *mut u8,
    ) -> i32,
    /// The size of the file read into `dst` (`cap` bytes), or an `E_*` code.
    pub read_all:
        unsafe extern "C" fn(root: u32, path: *const u8, len: u32, dst: *mut u8, cap: u32) -> i32,
    pub list_state_bytes: extern "C" fn(root: u32) -> u32,
    pub list_open: unsafe extern "C" fn(root: u32, dir: *const u8, len: u32, state: *mut u8) -> i32,
    /// 0 an entry (in `*out`), 1 pending, 2 end, or an `E_*` code.
    pub list_next: unsafe extern "C" fn(state: *mut u8, out: *mut DirEntry) -> i32,
}
