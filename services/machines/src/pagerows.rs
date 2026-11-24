//! The page rows of a device that keeps its machines' knobs in page rows (the Digitone II): the rows the registry reserves for run-time machines (`rows_info`, `PG_DYN_N` of
//! them from `PG_DYN0`). A free row names a machine no type has (`PG_FREE`), no knobs and the empty string as name and title. `init` puts every reserved row in that state when the
//! service starts: it was the page half of `mf_ensure` in the image (the names of the rows as `std::string` representations); `free_row` is the same
//! state for a row a module gives back (`unload::remove`).
use crate::registry::KNOBS;
use core::ptr::addr_of;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::mem::iw;
use dkrt_svc_rows::rows_info::{w, RegState};

/// The address of the reserved page row `k` (0 when the device has no page rows).
pub(crate) unsafe fn row_of(inf: *const u32, k: usize) -> u32 {
    if iw(inf, w::PG_ROWS) == 0 {
        return 0;
    }
    iw(inf, w::PG_ROWS)
        + (iw(inf, w::PG_DYN0) + k as u32 - iw(inf, w::PG_FIRST)) * iw(inf, w::PG_ROW_SIZE)
}

/// Reserved page row `k` as a free row: unbound from its machine first, then the rest of the row.
pub(crate) unsafe fn free_row<C: CoreApi>(core: &C, s: &mut RegState, inf: *const u32, k: usize) {
    let row = row_of(inf, k);
    if row == 0 {
        return;
    }
    core.wr(row + iw(inf, w::PGO_MACH), iw(inf, w::PG_FREE));
    s.empty_rep = [0, 0, 0xffff_ffff, 0];
    let rep = addr_of!(s.empty_rep) as u32 + 12;
    core.wr(row + iw(inf, w::PGO_NAME), rep);
    core.wr(row + iw(inf, w::PGO_TITLE), rep);
    let (c0, s0) = (row + iw(inf, w::PGO_C), row + iw(inf, w::PGO_S));
    let mut j = 0;
    while j < KNOBS {
        core.wr(c0 + 4 * j as u32, 0);
        core.wr(s0 + 4 * j as u32, 0);
        j += 1;
    }
    core.wr(c0 + 4 * KNOBS as u32, 10); // the track level code, as in a free row
    core.wr(s0 + 4 * KNOBS as u32, 0);
}

/// Every reserved page row is a free row. Nothing on a device without page rows or an image without the registry.
///
/// # Safety
/// The registry's words come from the image through the core; no module has filled a row (the service has just started).
pub(crate) unsafe fn init<C: CoreApi>(core: &C, s: &mut RegState) {
    let inf = dkrt_svc_rows::rows_info(core);
    if inf.is_null() {
        return;
    }
    let mut k = 0;
    while k < iw(inf, w::PG_DYN_N) as usize {
        free_row(core, s, inf, k);
        k += 1;
    }
}

#[cfg(test)]
mod tests;
