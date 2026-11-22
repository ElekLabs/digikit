//! `BootFns`: the boot record writers.
use crate::boot;
use dkrt_abi::module::LOG_NAME;
use dkrt_abi::resident::BootFns;
use dkrt_sdk::{NEEDS_LEN, SEL_MAX};

unsafe extern "C" fn note_boot_path(k: u32, path: *const u8) {
    boot::note_boot_path(k as usize, path)
}
unsafe extern "C" fn report_module(k: u32, path: *const u8, result: i32, missing: *const u8, name: *const u8) {
    boot::report_module(k as usize, path, result, &*(missing as *const [u8; NEEDS_LEN]), &*(name as *const [u8; LOG_NAME]))
}
unsafe extern "C" fn report_provider(path: *const u8, len: u32, result: i32, hash: u32, base: u32) {
    boot::report_provider(core::slice::from_raw_parts(path, len as usize), result, hash, base)
}
unsafe extern "C" fn report_dsp(idx: u32, result: i32) {
    boot::report_dsp(idx, result)
}
unsafe extern "C" fn report_taken_down(idx: u32, code: i32) {
    boot::report_taken_down(idx, code)
}
unsafe extern "C" fn begin_live(k: u32) {
    boot::begin_live(k as usize)
}
unsafe extern "C" fn restore_live(k: u32) {
    boot::restore_live(k as usize)
}
unsafe extern "C" fn unloaded(k: u32) {
    boot::unloaded(k as usize)
}
unsafe extern "C" fn live_index(path: *const u8, live: *const bool, out: *mut u32) -> i32 {
    match boot::live_index(path, &*(live as *const [bool; SEL_MAX])) {
        Ok(i) => {
            *out = i as u32;
            0
        }
        Err(e) => e,
    }
}
unsafe extern "C" fn account_load(k: u32, path: *const u8, result: i32, missing: *const u8, name: *const u8) {
    boot::account_load(k as usize, path, result, *(missing as *const [u8; NEEDS_LEN]), &*(name as *const [u8; LOG_NAME]))
}

pub(super) static FNS: BootFns = BootFns {
    note_boot_path,
    report_module,
    report_provider,
    report_dsp,
    report_taken_down,
    begin_live,
    restore_live,
    unloaded,
    live_index,
    account_load,
};
