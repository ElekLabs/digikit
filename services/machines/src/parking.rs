//! Parking of absent machine numbers: a number the sticky map holds that no module has reads MISSING and plays silence; the state of the passes, and the events for a number without a placeholder.
use crate::dsp;
use crate::registry::{rstate, MAX_MACH};
use crate::selection_view::refresh_view;
use crate::unload::reset_machine_row;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::mem::iw;
use dkrt_svc_rows::dynreg::w;

/// The machine rows (bit k) of a pinned number that a reload left as a MISSING placeholder: the new version's rows clear the bit, and the end of the reload writes a gap row
/// for the rest (a MISSING row with no voice row would play the first stock machine's sound). A number the sticky map holds stays MISSING and is not in this mask.
pub(crate) static mut MISSING_ROWS: u32 = 0;

/// How many passes in a row the DSP did not take a placeholder write (`park_absent`); it stops trying at `PARK_MAX` until a removal or the end of a reload.
pub(crate) static mut PARK_TRIES: u32 = 0;
const PARK_MAX: u32 = 3;

/// The machine rows (bit k) whose number has no placeholder and was logged as such (`MACHINE_TYPE_EVICTED`, an error event): one event for each time a number loses its
/// placeholder, not one for each pass.
pub(crate) static mut LOGGED: u32 = 0;

/// Why a number holds no placeholder (the third argument of the `MACHINE_TYPE_EVICTED` event of the machines service: hash, type, why): a real load took the row, no voice row is free,
/// the DSP did not take the write `PARK_MAX` passes in a row, the loader was busy `BUSY_MAX` passes in a row.
const WHY_GAVE_WAY: u32 = 1;
const WHY_NO_ROW: u32 = 2;
const WHY_WRITE_FAILED: u32 = 3;
const WHY_LOADER_BUSY: u32 = 4;

/// Passes in a row with the DSP loader busy after which a number that waits for its voice row is logged (why 4); the wait goes on and the row is parked when the loader is idle.
const BUSY_MAX: u32 = 1800;
pub(crate) static mut BUSY_PASSES: u32 = 0;

unsafe fn log_gap<C: CoreApi>(core: &C, k: usize, t: u32, why: u32) {
    if LOGGED & (1 << k) == 0 {
        LOGGED |= 1 << k;
        core.log(
            dkrt_sdk::logk::MACHINE_TYPE_EVICTED,
            dkrt_sdk::logk::F_ERROR,
            [crate::assign::hash_of(t), t, why, 0, 0],
        );
    }
}

/// Whether the machine row of type `t` reads MISSING: hidden from the list, `own_dsp` 1.
unsafe fn reads_missing<C: CoreApi>(core: &C, inf: *const u32, t: u32) -> bool {
    let row = iw(inf, w::M_ROWS) + (t - iw(inf, w::M_FIRST)) * iw(inf, w::M_ROW_SIZE);
    core.rd(row + iw(inf, w::O_NAME)) == iw(inf, w::EMPTY) && core.rd(row + iw(inf, w::O_OWN)) == 1
}

/// A real load took the placeholder of type `t` (`dsp::make_room`): its machine row falls back to the gap row (the stock sound) and the event says so; the number is parked
/// again when a row frees (`park_absent`).
pub(crate) unsafe fn give_way<C: CoreApi>(core: &C, t: u32) {
    let inf = dkrt_svc_rows::dynreg(core);
    let d0 = iw(inf, w::M_DYN0);
    if inf.is_null() || t < d0 || (t - d0) as usize >= MAX_MACH {
        return;
    }
    let k = (t - d0) as usize;
    if rstate().mach_owner[k] == 0 {
        reset_machine_row(core, inf, k, false);
        log_gap(core, k, t, WHY_GAVE_WAY);
    }
}

/// The machine rows and voice rows of the numbers the sticky map holds that no module has: MISSING and silent (docs/plans/extension-model.md E5d). A module that is not
/// loaded at boot, or failed before its rows were written, has no unload to leave them. Called from every UI-loop pass. The voice row is parked when the boot's loads are over
/// (`boot_index` is `u32::MAX` again) and the DSP loader is idle, a free row exists and the write is taken; only then the machine row reads MISSING. A number that cannot be
/// parked (no free row, or the write failed `PARK_MAX` passes in a row) keeps the gap row and one error event says why. Nothing runs during a live operation.
pub(crate) unsafe fn park_absent<C: CoreApi>(core: &C) {
    let inf = dkrt_svc_rows::dynreg(core);
    if inf.is_null() || core.live_op() != 0 || core.boot_index() != u32::MAX {
        return;
    }
    let s = rstate();
    let mut changed = false;
    let idle = dsp::loader_idle(core);
    BUSY_PASSES = if idle {
        0
    } else {
        BUSY_PASSES.saturating_add(1)
    };
    let mut failed = false;
    let mut i = 0;
    while i < dkrt_sdk::sel::MAP_N {
        let t = crate::assign::number_at(i);
        i += 1;
        let d0 = iw(inf, w::M_DYN0);
        if t < d0 || (t - d0) as usize >= MAX_MACH {
            continue;
        }
        let k = (t - d0) as usize;
        if s.mach_owner[k] != 0 || dsp::module_of_type(core, t).is_some() {
            continue;
        }
        let mut parked = dsp::has_voice_row(core, t);
        let mut why = 0;
        if !parked {
            if PARK_TRIES >= PARK_MAX {
                why = WHY_WRITE_FAILED;
            } else if !idle && BUSY_PASSES >= BUSY_MAX {
                why = WHY_LOADER_BUSY;
            } else if idle && !failed {
                match dsp::park_missing(core, t) {
                    Some(true) => {
                        parked = true;
                        PARK_TRIES = 0;
                    }
                    Some(false) => why = WHY_NO_ROW,
                    None => {
                        failed = true;
                        PARK_TRIES += 1;
                        if PARK_TRIES >= PARK_MAX {
                            why = WHY_WRITE_FAILED;
                        }
                    }
                }
            }
        }
        if parked {
            LOGGED &= !(1 << k);
            if !reads_missing(core, inf, t) {
                reset_machine_row(core, inf, k, true);
                changed = true;
            }
        } else if why != 0 {
            if reads_missing(core, inf, t) {
                reset_machine_row(core, inf, k, false);
                changed = true;
            }
            log_gap(core, k, t, why);
        }
    }
    if changed {
        refresh_view(core);
    }
}
