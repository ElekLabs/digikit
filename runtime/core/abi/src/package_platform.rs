//! `PackagePlatform`: the `Platform` methods the system package uses (the 17 of the inventory, `sample_path_max` and the device facts the capacity report reads), and nothing else of the device (docs/plans/rc2s-1-boundary-inventory.md, section 3.3).
//!
//! Package code takes `P: PackagePlatform` where it needs the device, so what it may ask of the resident core is this list and no more. The resident platform
//! (`RealPlatform`, and `FakePlatform` in host tests) gives it through its `Platform`: the impl below forwards each method, so no platform writes it twice. A package in
//! its own artifact (RC2s-2) implements it over the resident table instead.
//!
//! Not part of the service ABI: a service never sees it (the file is not one of `SVC_ABI_FILES`).

use crate::dev_addr::DevAddr;
use crate::platform::{Platform, Tasking};
use crate::storage::{DirEntry, ListStep, Stat, Storage};

pub trait PackagePlatform {
    /// The firmware's heap: null when it is out of memory.
    unsafe fn alloc(&self, bytes: u32) -> *mut u8;
    unsafe fn free(&self, p: *mut u8);
    /// The firmware's DSP block counter; `None` when it is not available.
    fn blocks(&self) -> Option<u32>;
    /// Mask interrupts; the old status register.
    fn irq_mask(&self) -> u32;
    fn irq_restore(&self, sr: u32);
    /// Write back and invalidate both caches over a byte range (before running fresh code).
    fn flush(&self, start: u32, len: u32);
    /// The address of the image's info entry `name_hash`; null when it has none.
    unsafe fn info(&self, name_hash: u32) -> *const u32;
    /// The device address of the `[[slot]]` word `name_hash`; `NULL` for any other entry.
    unsafe fn info_slot(&self, name_hash: u32) -> DevAddr;
    /// The index in core/dsp-link's slot block of the DSP slot `name_hash`.
    unsafe fn info_dsp_slot(&self, name_hash: u32) -> Option<u32>;
    /// Read `out.len()` bytes of code at the raw address `addr`.
    unsafe fn read_code(&self, addr: u32, out: &mut [u8]);
    /// Write `bytes` at the raw address `addr`.
    unsafe fn write_code(&self, addr: u32, bytes: &[u8]);
    /// Store one slot word at the 4-aligned address `addr`.
    unsafe fn write_word(&self, addr: u32, value: u32);
    /// The firmware's transport state; `None` when the platform cannot read it.
    unsafe fn play_state(&self) -> Option<u32>;
    /// The `[digikit] id` of the device.
    fn device_id(&self) -> u32;
    /// The longest path of a sample file of the device.
    fn sample_path_max(&self) -> u32;
    /// The device has the DSP the digikit link drives.
    fn has_dsp(&self) -> bool;
    /// The DSP load the device allows, in the units of the CAPACITY entry (`DeviceProfile::dsp_headroom`).
    fn dsp_headroom(&self) -> u32;
    /// The voices the device's DSP runs (`DeviceProfile::dsp_voices`).
    fn dsp_voices(&self) -> u32;
    /// The DSP cycles of one block (`DeviceProfile::dsp_block_cycles`).
    fn dsp_block_cycles(&self) -> u32;
    /// Kind, size and hash of the store or sample path.
    unsafe fn stat(&self, root: u32, path: &[u8]) -> Result<Stat, i32>;
    /// `stat` and the first 64 bytes of a file.
    unsafe fn stat_head(
        &self,
        root: u32,
        path: &[u8],
        out: &mut [u8; 64],
    ) -> Result<(Stat, usize), i32>;
    /// Read a whole file into `dst`, which holds `cap` bytes; the file size.
    unsafe fn read_all(&self, root: u32, path: &[u8], dst: *mut u8, cap: u32) -> Result<u32, i32>;
    /// Bytes of listing state for `root` (`Storage::list_state_bytes`).
    fn list_state_bytes(&self, root: u32) -> usize;
    /// Start a listing of `dir` in the block `state` (`Storage::list_open`).
    unsafe fn list_open(&self, root: u32, dir: &[u8], state: *mut u8) -> Result<(), i32>;
    /// One bounded listing step (`Storage::list_next`).
    unsafe fn list_next(&self, state: *mut u8, out: &mut DirEntry) -> Result<ListStep, i32>;

    // ---- provided ------------------------------------------------------------------------------------------------------------
    /// Blocks from `start` to now, wrapping; `None` when the clock was or is not available (`platform::blocks_since`, which the resident core's worker calls on a `Platform`).
    #[inline(always)]
    fn blocks_since(&self, start: Option<u32>) -> Option<u32> {
        match (start, self.blocks()) {
            (Some(a), Some(b)) => Some(b.wrapping_sub(a)),
            _ => None,
        }
    }
}

impl<P: Platform> PackagePlatform for P {
    #[inline(always)]
    unsafe fn alloc(&self, bytes: u32) -> *mut u8 {
        Tasking::alloc(self, bytes)
    }
    #[inline(always)]
    unsafe fn free(&self, p: *mut u8) {
        Tasking::free(self, p)
    }
    #[inline(always)]
    fn blocks(&self) -> Option<u32> {
        Platform::blocks(self)
    }
    #[inline(always)]
    fn irq_mask(&self) -> u32 {
        Platform::irq_mask(self)
    }
    #[inline(always)]
    fn irq_restore(&self, sr: u32) {
        Platform::irq_restore(self, sr)
    }
    #[inline(always)]
    fn flush(&self, start: u32, len: u32) {
        Platform::flush(self, start, len)
    }
    #[inline(always)]
    unsafe fn info(&self, name_hash: u32) -> *const u32 {
        Platform::info(self, name_hash)
    }
    #[inline(always)]
    unsafe fn info_slot(&self, name_hash: u32) -> DevAddr {
        Platform::info_slot(self, name_hash)
    }
    #[inline(always)]
    unsafe fn info_dsp_slot(&self, name_hash: u32) -> Option<u32> {
        Platform::info_dsp_slot(self, name_hash)
    }
    #[inline(always)]
    unsafe fn read_code(&self, addr: u32, out: &mut [u8]) {
        Platform::read_code(self, addr, out)
    }
    #[inline(always)]
    unsafe fn write_code(&self, addr: u32, bytes: &[u8]) {
        Platform::write_code(self, addr, bytes)
    }
    #[inline(always)]
    unsafe fn write_word(&self, addr: u32, value: u32) {
        Platform::write_word(self, addr, value)
    }
    #[inline(always)]
    unsafe fn play_state(&self) -> Option<u32> {
        Platform::play_state(self)
    }
    #[inline(always)]
    fn device_id(&self) -> u32 {
        Platform::device_id(self)
    }
    #[inline(always)]
    fn sample_path_max(&self) -> u32 {
        Platform::sample_path_max(self)
    }
    #[inline(always)]
    fn has_dsp(&self) -> bool {
        Platform::has_dsp(self)
    }
    #[inline(always)]
    fn dsp_headroom(&self) -> u32 {
        Platform::dsp_headroom(self)
    }
    #[inline(always)]
    fn dsp_voices(&self) -> u32 {
        Platform::dsp_voices(self)
    }
    #[inline(always)]
    fn dsp_block_cycles(&self) -> u32 {
        Platform::dsp_block_cycles(self)
    }
    #[inline(always)]
    unsafe fn stat(&self, root: u32, path: &[u8]) -> Result<Stat, i32> {
        Storage::stat(self, root, path)
    }
    #[inline(always)]
    unsafe fn stat_head(
        &self,
        root: u32,
        path: &[u8],
        out: &mut [u8; 64],
    ) -> Result<(Stat, usize), i32> {
        Storage::stat_head(self, root, path, out)
    }
    #[inline(always)]
    unsafe fn read_all(&self, root: u32, path: &[u8], dst: *mut u8, cap: u32) -> Result<u32, i32> {
        Storage::read_all(self, root, path, dst, cap)
    }
    #[inline(always)]
    fn list_state_bytes(&self, root: u32) -> usize {
        Storage::list_state_bytes(self, root)
    }
    #[inline(always)]
    unsafe fn list_open(&self, root: u32, dir: &[u8], state: *mut u8) -> Result<(), i32> {
        Storage::list_open(self, root, dir, state)
    }
    #[inline(always)]
    unsafe fn list_next(&self, state: *mut u8, out: &mut DirEntry) -> Result<ListStep, i32> {
        Storage::list_next(self, state, out)
    }
}
