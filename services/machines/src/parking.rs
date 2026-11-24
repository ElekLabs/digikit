//! Parking of absent machine numbers: a number the sticky map holds that no module has reads MISSING and plays silence; the state of the passes, and the events for a number without a placeholder.
use crate::dsp;
use crate::registry::{rstate, MAX_MACH};
use crate::selection_view::refresh_view;
use crate::unload::reset_machine_row;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::mem::iw;
use dkrt_svc_rows::dynreg::w;

#[cfg(test)]
mod tests;

const PARK_MAX: u32 = 3;

/// Why a number holds no placeholder (the third argument of the `MACHINE_TYPE_EVICTED` event of the machines service: hash, type, why): a real load took the row, no voice row is free,
/// the DSP did not take the write `PARK_MAX` passes in a row, the loader was busy `BUSY_MAX` passes in a row.
const WHY_GAVE_WAY: u32 = 1;
const WHY_NO_ROW: u32 = 2;
const WHY_WRITE_FAILED: u32 = 3;
const WHY_LOADER_BUSY: u32 = 4;

/// Passes in a row with the DSP loader busy after which a number that waits for its voice row is logged (why 4); the wait goes on and the row is parked when the loader is idle.
const BUSY_MAX: u32 = 1800;

/// The state of the parking passes: it belongs to the owner, and is reset by the plan of the numbers (`settle`) and the end of a reload.
pub struct Parking {
    /// The machine rows (bit k) of a pinned number that a reload left as a MISSING placeholder: the new version's rows clear the bit, and the end of the reload writes a gap
    /// row for the rest (a MISSING row with no voice row would play the first stock machine's sound). A number the sticky map holds stays MISSING and is not in this mask.
    missing_rows: u32,
    /// How many passes in a row the DSP did not take a placeholder write (`park_absent`); it stops trying at `PARK_MAX` until a removal or the end of a reload.
    tries: u32,
    /// The machine rows (bit k) whose number has no placeholder and was logged as such (`MACHINE_TYPE_EVICTED`, an error event): one event for each time a number loses its
    /// placeholder, not one for each pass.
    logged: u32,
    /// Passes in a row with the DSP loader busy.
    busy_passes: u32,
}

impl Default for Parking {
    fn default() -> Parking {
        Parking::new()
    }
}

impl Parking {
    pub const fn new() -> Parking {
        Parking {
            missing_rows: 0,
            tries: 0,
            logged: 0,
            busy_passes: 0,
        }
    }

    /// The parking tries in a row, the rows logged as without a placeholder (bit k) and the passes with a busy loader (read only: the tests and the owner's diagnostics).
    pub fn tries(&self) -> u32 {
        self.tries
    }
    pub fn logged(&self) -> u32 {
        self.logged
    }
    pub fn busy_passes(&self) -> u32 {
        self.busy_passes
    }

    /// A new plan of the numbers: the parking state of the earlier one is not carried over.
    pub(crate) fn plan_changed(&mut self) {
        self.tries = 0;
        self.busy_passes = 0;
        self.logged = 0;
    }

    /// The machine rows (bit k) a reload left as a MISSING placeholder.
    pub fn missing_rows(&self) -> u32 {
        self.missing_rows
    }

    /// The end of a reload: no row is waiting for its new version, and the parking tries start again.
    pub(crate) fn reload_over(&mut self) {
        self.missing_rows = 0;
        self.tries = 0;
    }

    /// Machine row `k` was filled by a module: it is no placeholder and has nothing logged.
    pub(crate) fn row_filled(&mut self, k: usize) {
        self.missing_rows &= !(1 << k);
        self.logged &= !(1 << k);
    }

    /// Machine row `k` was given back (`remove`): `missing` when it keeps reading MISSING while its module reloads; the parking tries start again.
    pub(crate) fn row_removed(&mut self, k: usize, missing: bool) {
        if missing {
            self.missing_rows |= 1 << k;
        }
        self.tries = 0;
    }

    unsafe fn log_gap<C: CoreApi>(&mut self, core: &C, k: usize, t: u32, why: u32) {
        if self.logged & (1 << k) == 0 {
            self.logged |= 1 << k;
            core.log(
                dkrt_sdk::logk::MACHINE_TYPE_EVICTED,
                dkrt_sdk::logk::F_ERROR,
                [crate::assign::hash_of(t), t, why, 0, 0],
            );
        }
    }

    /// A real load took the placeholder of type `t` (`dsp::make_room`): its machine row falls back to the gap row (the stock sound) and the event says so; the number is parked
    /// again when a row frees (`park_absent`).
    pub(crate) unsafe fn give_way<C: CoreApi>(&mut self, core: &C, t: u32) {
        let inf = dkrt_svc_rows::dynreg(core);
        let d0 = iw(inf, w::M_DYN0);
        if inf.is_null() || t < d0 || (t - d0) as usize >= MAX_MACH {
            return;
        }
        let k = (t - d0) as usize;
        if rstate().mach_owner[k] == 0 {
            reset_machine_row(core, inf, k, false);
            self.log_gap(core, k, t, WHY_GAVE_WAY);
        }
    }

    /// The machine rows and voice rows of the numbers the sticky map holds that no module has: MISSING and silent (docs/plans/extension-model.md E5d). A module that is not
    /// loaded at boot, or failed before its rows were written, has no unload to leave them. Called from every UI-loop pass. The voice row is parked when the boot's loads are over
    /// (`boot_index` is `u32::MAX` again) and the DSP loader is idle, a free row exists and the write is taken; only then the machine row reads MISSING. A number that cannot be
    /// parked (no free row, or the write failed `PARK_MAX` passes in a row) keeps the gap row and one error event says why. Nothing runs during a live operation.
    pub(crate) unsafe fn park_absent<C: CoreApi>(&mut self, core: &C) {
        let inf = dkrt_svc_rows::dynreg(core);
        if inf.is_null() || core.live_op() != 0 || core.boot_index() != u32::MAX {
            return;
        }
        let s = rstate();
        let mut changed = false;
        let idle = dsp::loader_idle(core);
        self.busy_passes = if idle {
            0
        } else {
            self.busy_passes.saturating_add(1)
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
                if self.tries >= PARK_MAX {
                    why = WHY_WRITE_FAILED;
                } else if !idle && self.busy_passes >= BUSY_MAX {
                    why = WHY_LOADER_BUSY;
                } else if idle && !failed {
                    match dsp::park_missing(core, t) {
                        Some(true) => {
                            parked = true;
                            self.tries = 0;
                        }
                        Some(false) => why = WHY_NO_ROW,
                        None => {
                            failed = true;
                            self.tries += 1;
                            if self.tries >= PARK_MAX {
                                why = WHY_WRITE_FAILED;
                            }
                        }
                    }
                }
            }
            if parked {
                self.logged &= !(1 << k);
                if !reads_missing(core, inf, t) {
                    reset_machine_row(core, inf, k, true);
                    changed = true;
                }
            } else if why != 0 {
                if reads_missing(core, inf, t) {
                    reset_machine_row(core, inf, k, false);
                    changed = true;
                }
                self.log_gap(core, k, t, why);
            }
        }
        if changed {
            refresh_view(core);
        }
    }
}

/// Whether the machine row of type `t` reads MISSING: hidden from the list, `own_dsp` 1.
unsafe fn reads_missing<C: CoreApi>(core: &C, inf: *const u32, t: u32) -> bool {
    let row = iw(inf, w::M_ROWS) + (t - iw(inf, w::M_FIRST)) * iw(inf, w::M_ROW_SIZE);
    core.rd(row + iw(inf, w::O_NAME)) == iw(inf, w::EMPTY) && core.rd(row + iw(inf, w::O_OWN)) == 1
}
