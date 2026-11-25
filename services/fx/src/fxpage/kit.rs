//! The live kit object and the value array it holds.

use super::*;
use core::ptr::{addr_of_mut, read_volatile};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::mem::iw;

/// Kit loads and kit initialisations the hooks of the effect pages have done (their counters move when a kit's rows are replaced).
#[inline]
pub(super) fn hook_runs() -> u32 {
    unsafe {
        read_volatile(addr_of_mut!(DKRT_FXP_NLOAD))
            .wrapping_add(read_volatile(addr_of_mut!(DKRT_FXP_NINIT)))
    }
}

/// Put the mark of the effect pages into the kit array `arr`: its effect rows hold values.
#[inline]
pub(super) unsafe fn mark<C: CoreApi>(core: &C, inf: *const u32, arr: u32) {
    core.wr16(
        arr + 2 * iw(inf, w::MARK_INDEX),
        iw(inf, w::MARK_VALUE) as i32,
    );
}

/// The defaults of effect row `k` into the kit array `arr`.
pub(super) unsafe fn seed_row<C: CoreApi>(core: &C, inf: *const u32, k: usize, arr: u32) {
    let s = fs();
    let mut j = 0;
    while j < WORDS {
        if has_word(s, k, j) {
            core.wr16(arr + 2 * value_index(inf, k, j), s.def[k][j]);
            s.cur[k][j] = s.def[k][j];
        }
        j += 1;
    }
}

/// Is `a` in the cached DDR window of the device the runtime runs on? The window is a device fact: the profile of the device id (the generated table). A device
/// without a profile has no window.
fn in_ddr<C: CoreApi>(core: &C, a: u32) -> bool {
    dkrt_abi::platform::DeviceProfile::by_id(core.device_id()).is_some_and(|p| p.ddr_contains(a))
}

/// The live kit object (0 while the firmware has none); a pointer in a DDR cell, checked to lie in DDR. Any alignment: the Digitone II's kit objects are allocated at odd
/// addresses (its sound arrays have an odd stride), so the array's halfwords are read and written a byte at a time.
pub(crate) unsafe fn kit<C: CoreApi>(core: &C, inf: *const u32) -> u32 {
    let k = core.rd(iw(inf, w::KIT_PTR));
    if !in_ddr(core, k) {
        return 0;
    }
    k
}
