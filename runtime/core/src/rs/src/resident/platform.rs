//! `PlatformFns` over the real platform. `irq_mask`, `irq_restore`, `flush` and `has_dsp` are the platform's own `extern "C"` functions.
use crate::platform::PLATFORM;
use core::slice::from_raw_parts;
use dkrt_abi::package_platform::PackagePlatform;
use dkrt_abi::resident::{Group, PlatformFns};
use dkrt_abi::storage::{DirEntry, ListStep, StatWire};

unsafe fn text<'a>(p: *const u8, n: u32) -> &'a [u8] {
    if n == 0 {
        &[]
    } else {
        from_raw_parts(p, n as usize)
    }
}

unsafe extern "C" fn alloc(bytes: u32) -> *mut u8 {
    PackagePlatform::alloc(&PLATFORM, bytes)
}
unsafe extern "C" fn free(p: *mut u8) {
    PackagePlatform::free(&PLATFORM, p)
}
unsafe extern "C" fn blocks(out: *mut u32) -> i32 {
    match PackagePlatform::blocks(&PLATFORM) {
        Some(b) => {
            *out = b;
            0
        }
        None => -1,
    }
}
unsafe extern "C" fn info(name_hash: u32) -> *const u32 {
    PackagePlatform::info(&PLATFORM, name_hash)
}
unsafe extern "C" fn info_slot(name_hash: u32) -> u32 {
    PackagePlatform::info_slot(&PLATFORM, name_hash).get()
}
unsafe extern "C" fn info_dsp_slot(name_hash: u32, out: *mut u32) -> i32 {
    match PackagePlatform::info_dsp_slot(&PLATFORM, name_hash) {
        Some(i) => {
            *out = i;
            0
        }
        None => -1,
    }
}
unsafe extern "C" fn read_code(addr: u32, out: *mut u8, len: u32) {
    PackagePlatform::read_code(&PLATFORM, addr, core::slice::from_raw_parts_mut(out, len as usize))
}
unsafe extern "C" fn write_code(addr: u32, bytes: *const u8, len: u32) {
    PackagePlatform::write_code(&PLATFORM, addr, text(bytes, len))
}
unsafe extern "C" fn write_word(addr: u32, value: u32) {
    PackagePlatform::write_word(&PLATFORM, addr, value)
}
unsafe extern "C" fn play_state(out: *mut u32) -> i32 {
    match PackagePlatform::play_state(&PLATFORM) {
        Some(s) => {
            *out = s;
            0
        }
        None => -1,
    }
}
extern "C" fn device_id() -> u32 {
    PackagePlatform::device_id(&PLATFORM)
}
extern "C" fn sample_path_max() -> u32 {
    PackagePlatform::sample_path_max(&PLATFORM)
}
extern "C" fn dsp_headroom() -> u32 {
    PackagePlatform::dsp_headroom(&PLATFORM)
}
extern "C" fn dsp_voices() -> u32 {
    PackagePlatform::dsp_voices(&PLATFORM)
}
extern "C" fn dsp_block_cycles() -> u32 {
    PackagePlatform::dsp_block_cycles(&PLATFORM)
}
unsafe extern "C" fn stat(root: u32, path: *const u8, len: u32, out: *mut StatWire) -> i32 {
    match PackagePlatform::stat(&PLATFORM, root, text(path, len)) {
        Ok(s) => {
            *out = StatWire::of(&s);
            0
        }
        Err(e) => e,
    }
}
unsafe extern "C" fn stat_head(root: u32, path: *const u8, len: u32, out: *mut StatWire, head: *mut u8) -> i32 {
    match PackagePlatform::stat_head(&PLATFORM, root, text(path, len), &mut *(head as *mut [u8; 64])) {
        Ok((s, n)) => {
            *out = StatWire::of(&s);
            n as i32
        }
        Err(e) => e,
    }
}
unsafe extern "C" fn read_all(root: u32, path: *const u8, len: u32, dst: *mut u8, cap: u32) -> i32 {
    match PackagePlatform::read_all(&PLATFORM, root, text(path, len), dst, cap) {
        Ok(n) => n as i32,
        Err(e) => e,
    }
}
extern "C" fn list_state_bytes(root: u32) -> u32 {
    PackagePlatform::list_state_bytes(&PLATFORM, root) as u32
}
unsafe extern "C" fn list_open(root: u32, dir: *const u8, len: u32, state: *mut u8) -> i32 {
    match PackagePlatform::list_open(&PLATFORM, root, text(dir, len), state) {
        Ok(()) => 0,
        Err(e) => e,
    }
}
unsafe extern "C" fn list_next(state: *mut u8, out: *mut DirEntry) -> i32 {
    match PackagePlatform::list_next(&PLATFORM, state, &mut *out) {
        Ok(ListStep::Entry) => 0,
        Ok(ListStep::Pending) => 1,
        Ok(ListStep::End) => 2,
        Err(e) => e,
    }
}

pub(super) static FNS: Group<PlatformFns> = Group::new(PlatformFns {
    alloc,
    free,
    blocks,
    irq_mask: crate::platform::irq_mask,
    irq_restore: crate::platform::irq_restore,
    flush: crate::platform::flush,
    info,
    info_slot,
    info_dsp_slot,
    read_code,
    write_code,
    write_word,
    play_state,
    device_id,
    sample_path_max,
    has_dsp: crate::platform::has_dsp,
    dsp_headroom,
    dsp_voices,
    dsp_block_cycles,
    stat,
    stat_head,
    read_all,
    list_state_bytes,
    list_open,
    list_next,
});
