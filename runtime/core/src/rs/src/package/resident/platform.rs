//! The device and the firmware under the core: `TablePlatform` is the package's `PackagePlatform`.
use super::table;
use dkrt_abi::dev_addr::DevAddr;
use dkrt_abi::package_platform::PackagePlatform;
use dkrt_abi::storage::{DirEntry, ListStep, Stat, StatWire};

/// The package's platform: every method forwards to the table.
#[derive(Clone, Copy)]
pub(crate) struct TablePlatform;

/// The value package code passes where it needs the platform.
pub(crate) const PLATFORM: TablePlatform = TablePlatform;

/// The pointer and length of a path for the table.
#[inline(always)]
fn raw(path: &[u8]) -> (*const u8, u32) {
    (path.as_ptr(), path.len() as u32)
}

impl PackagePlatform for TablePlatform {
    #[inline(always)]
    unsafe fn alloc(&self, bytes: u32) -> *mut u8 {
        (table().platform.alloc)(bytes)
    }
    #[inline(always)]
    unsafe fn free(&self, p: *mut u8) {
        (table().platform.free)(p)
    }
    #[inline(always)]
    fn blocks(&self) -> Option<u32> {
        let mut v = 0u32;
        if unsafe { (table().platform.blocks)(&mut v) } == 0 {
            Some(v)
        } else {
            None
        }
    }
    #[inline(always)]
    fn irq_mask(&self) -> u32 {
        (table().platform.irq_mask)()
    }
    #[inline(always)]
    fn irq_restore(&self, sr: u32) {
        (table().platform.irq_restore)(sr)
    }
    #[inline(always)]
    fn flush(&self, start: u32, len: u32) {
        (table().platform.flush)(start, len)
    }
    #[inline(always)]
    unsafe fn info(&self, name_hash: u32) -> *const u32 {
        (table().platform.info)(name_hash)
    }
    #[inline(always)]
    unsafe fn info_slot(&self, name_hash: u32) -> DevAddr {
        DevAddr::new((table().platform.info_slot)(name_hash))
    }
    #[inline(always)]
    unsafe fn info_dsp_slot(&self, name_hash: u32) -> Option<u32> {
        let mut v = 0u32;
        if (table().platform.info_dsp_slot)(name_hash, &mut v) == 0 {
            Some(v)
        } else {
            None
        }
    }
    #[inline(always)]
    unsafe fn read_code(&self, addr: u32, out: &mut [u8]) {
        (table().platform.read_code)(addr, out.as_mut_ptr(), out.len() as u32)
    }
    #[inline(always)]
    unsafe fn write_code(&self, addr: u32, bytes: &[u8]) {
        (table().platform.write_code)(addr, bytes.as_ptr(), bytes.len() as u32)
    }
    #[inline(always)]
    unsafe fn write_word(&self, addr: u32, value: u32) {
        (table().platform.write_word)(addr, value)
    }
    #[inline(always)]
    unsafe fn play_state(&self) -> Option<u32> {
        let mut v = 0u32;
        if (table().platform.play_state)(&mut v) == 0 {
            Some(v)
        } else {
            None
        }
    }
    #[inline(always)]
    fn device_id(&self) -> u32 {
        (table().platform.device_id)()
    }
    #[inline(always)]
    fn sample_path_max(&self) -> u32 {
        (table().platform.sample_path_max)()
    }
    #[inline(always)]
    fn has_dsp(&self) -> bool {
        (table().platform.has_dsp)()
    }
    #[inline(always)]
    fn dsp_headroom(&self) -> u32 {
        (table().platform.dsp_headroom)()
    }
    #[inline(always)]
    fn dsp_voices(&self) -> u32 {
        (table().platform.dsp_voices)()
    }
    #[inline(always)]
    fn dsp_block_cycles(&self) -> u32 {
        (table().platform.dsp_block_cycles)()
    }
    #[inline(always)]
    unsafe fn stat(&self, root: u32, path: &[u8]) -> Result<Stat, i32> {
        let mut w = StatWire::empty();
        let (p, n) = raw(path);
        let r = (table().platform.stat)(root, p, n, &mut w);
        if r == 0 {
            Ok(w.stat())
        } else {
            Err(r)
        }
    }
    #[inline(always)]
    unsafe fn stat_head(&self, root: u32, path: &[u8], out: &mut [u8; 64]) -> Result<(Stat, usize), i32> {
        let mut w = StatWire::empty();
        let (p, n) = raw(path);
        let r = (table().platform.stat_head)(root, p, n, &mut w, out.as_mut_ptr());
        if r >= 0 {
            Ok((w.stat(), r as usize))
        } else {
            Err(r)
        }
    }
    #[inline(always)]
    unsafe fn read_all(&self, root: u32, path: &[u8], dst: *mut u8, cap: u32) -> Result<u32, i32> {
        let (p, n) = raw(path);
        let r = (table().platform.read_all)(root, p, n, dst, cap);
        if r >= 0 {
            Ok(r as u32)
        } else {
            Err(r)
        }
    }
    #[inline(always)]
    fn list_state_bytes(&self, root: u32) -> usize {
        (table().platform.list_state_bytes)(root) as usize
    }
    #[inline(always)]
    unsafe fn list_open(&self, root: u32, dir: &[u8], state: *mut u8) -> Result<(), i32> {
        let (p, n) = raw(dir);
        let r = (table().platform.list_open)(root, p, n, state);
        if r == 0 {
            Ok(())
        } else {
            Err(r)
        }
    }
    #[inline(always)]
    unsafe fn list_next(&self, state: *mut u8, out: &mut DirEntry) -> Result<ListStep, i32> {
        match (table().platform.list_next)(state, out) {
            0 => Ok(ListStep::Entry),
            1 => Ok(ListStep::Pending),
            2 => Ok(ListStep::End),
            e => Err(e),
        }
    }
}

/// The digikit id of the device (a value for logs and replies).
#[inline(always)]
pub(crate) fn device() -> u32 {
    PLATFORM.device_id()
}

/// The device has the DSP the digikit link drives.
#[inline(always)]
pub(crate) fn has_dsp() -> bool {
    PLATFORM.has_dsp()
}

#[inline(always)]
pub(crate) fn irq_mask() -> u32 {
    PLATFORM.irq_mask()
}

#[inline(always)]
pub(crate) fn irq_restore(sr: u32) {
    PLATFORM.irq_restore(sr)
}

/// The firmware's DSP block counter: `None` when it is not available.
#[inline(always)]
pub(crate) fn fw_blocks() -> Option<u32> {
    PLATFORM.blocks()
}
