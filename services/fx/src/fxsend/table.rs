//! The info table of the send cells and the count cell of the track FX key.

use super::*;
use crate::fxpage;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::mem::iw;

/// `inf` (a bound table of the effect pages, or null) when this device has send cells (at least one), else null.
unsafe fn with_cells(inf: *const u32) -> *const u32 {
    if inf.is_null() || iw(inf, w::CELLS) == 0 {
        return core::ptr::null();
    }
    inf
}

/// The info table when this image has send cells, else null.
pub(super) unsafe fn info<C: CoreApi>(core: &C) -> *const u32 {
    with_cells(fxpage::info(core))
}

/// The cells of the send page this image has (the CAPACITY limit of `SEND_SLOTS`): 0 without effect pages in this image or on a device with no send page.
pub unsafe fn slots<C: CoreApi>(core: &C) -> u32 {
    cells(info(core))
}

/// The same from the table that `Fx::start` bound, for a call that has no core (the resident query).
pub fn bound_slots() -> u32 {
    // SAFETY: the bound table is this crate's static, null before `bind`
    unsafe { cells(with_cells(fxpage::bound())) }
}

unsafe fn cells(inf: *const u32) -> u32 {
    if inf.is_null() {
        return 0;
    }
    iw(inf, w::CELLS).min(SEND_MAX as u32)
}

/// The count cell of the track FX key's list: 2 (the send page follows the stock page) while a cell is in use, else 0 (the stock list).
#[inline]
pub(super) unsafe fn publish_count<C: CoreApi>(core: &C, inf: *const u32) {
    core.wr(iw(inf, w::COUNT), if ss().used != 0 { 2 } else { 0 });
}
