//! The boot record the resident core keeps: what the package reports as it loads, takes down and reloads modules.
use super::table;
use dkrt_abi::module::LOG_NAME;
use dkrt_abi::state::{BootStates, LoadsView};
use dkrt_sdk::{DkrtBootInfo, NEEDS_LEN, SEL_MAX};

/// `DKRT_BOOT`.
#[inline(always)]
pub(crate) fn info() -> &'static mut DkrtBootInfo {
    unsafe { &mut *table().views.boot_info }
}

/// The states of this boot's modules (`dkrt_abi::state`): a view of `DKRT_BOOT.states` and `.detail`.
#[inline(always)]
pub(crate) fn states() -> BootStates<'static> {
    let b = info();
    BootStates { states: &mut b.states, detail: &mut b.detail }
}

/// The module records `LOADS` reports, as `dkrt_abi::state::LoadsView`.
#[inline(always)]
pub(crate) fn loads_view() -> LoadsView<'static> {
    let b = info();
    LoadsView { count: b.count as usize, results: &b.results, states: &b.states, detail: &b.detail, needs: &b.needs, names: unsafe { &*table().views.boot_names } }
}

/// Paths of the selection this boot loaded (for CAPACITY).
#[inline(always)]
pub(crate) fn selected() -> u32 {
    info().count
}

/// The boot's load loop reads selection entry `k` with path `path`.
#[inline(always)]
pub(crate) unsafe fn note_boot_path(k: usize, path: *const u8) {
    (table().boot.note_boot_path)(k as u32, path)
}

/// The index a live job loads the file at `path` into.
#[inline(always)]
pub(crate) unsafe fn live_index(path: *const u8, live: &[bool; SEL_MAX]) -> Result<usize, i32> {
    let mut at = 0u32;
    let r = (table().boot.live_index)(path, live.as_ptr(), &mut at);
    if r == 0 {
        Ok(at as usize)
    } else {
        Err(r)
    }
}

#[inline(always)]
pub(crate) unsafe fn restore_live(k: usize) {
    (table().boot.restore_live)(k as u32)
}

#[inline(always)]
pub(crate) unsafe fn begin_live(k: usize) {
    (table().boot.begin_live)(k as u32)
}

#[inline(always)]
pub(crate) unsafe fn unloaded(k: usize) {
    (table().boot.unloaded)(k as u32)
}

#[inline(always)]
pub(crate) unsafe fn report_module(k: usize, path: *const u8, res: i32, missing: &[u8; NEEDS_LEN], name: &[u8; LOG_NAME]) {
    (table().boot.report_module)(k as u32, path, res, missing.as_ptr(), name.as_ptr())
}

#[inline(always)]
pub(crate) unsafe fn account_load(k: usize, path: *const u8, r: i32, missing: [u8; NEEDS_LEN], name: &[u8; LOG_NAME]) {
    (table().boot.account_load)(k as u32, path, r, missing.as_ptr(), name.as_ptr())
}

#[inline(always)]
pub(crate) unsafe fn report_provider(path: &[u8], res: i32, hash: u32, base: u32) {
    (table().boot.report_provider)(path.as_ptr(), path.len() as u32, res, hash, base)
}

#[inline(always)]
pub(crate) unsafe fn report_dsp(idx: u32, r: i32) {
    (table().boot.report_dsp)(idx, r)
}

#[inline(always)]
pub(crate) unsafe fn report_taken_down(idx: u32, code: i32) {
    (table().boot.report_taken_down)(idx, code)
}
