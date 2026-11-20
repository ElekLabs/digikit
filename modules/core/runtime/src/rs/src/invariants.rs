//! The consistency check of the runtime's tables (docs/design/module-sdk.md 6.1): one table of module records, with the ColdFire
//! arena ranges, the DSP area ranges, the voice rows and the registry rows each tied to the record that owns them.
//!
//! `check` runs at the end of every load attempt and every unload (`loader`). It reads the tables and changes nothing but `TABLE_BAD`
//! and one log event. Once a check has failed, the loader refuses every new load with `E_TABLE`.
//!
//! The invariants (the number is what `check` returns and the `TABLE_BAD` event carries):
//! 1. a used record's ColdFire range lies inside the arena, and its DSP range (when it has one) inside the DSP area;
//! 2. the ColdFire ranges of used records are pairwise disjoint, and so are their DSP ranges;
//! 3. every owned voice row belongs to a used record whose `mod_rows` has the row's bit, and every bit of a used record's `mod_rows`
//!    is a row it owns; an unused record has no DSP range, no rows and no registry rows, and every registry row has a used owner;
//!    the tables of the services agree with the record table (`Service::table_check`: the effect rows follow the same rule, a bit of `mod_fx` for each row of the
//!    chain a record owns).
//! 4. no two used records have the same name.
use crate::{cstr_eq, dlog, dsp, st, ARENA_BYTES, DKRT_ARENA, MAX_MODULES};
use core::ptr::{addr_of, addr_of_mut};
use dkrt_sdk::*;

/// 1 once a check has failed: the loader then refuses every load. Named so a host can peek it (`DKRT_TABLE_BAD` in plan.json symbols).
#[no_mangle]
pub static mut DKRT_TABLE_BAD: u8 = 0;

pub fn bad() -> bool {
    unsafe { *addr_of!(DKRT_TABLE_BAD) != 0 }
}

unsafe fn fail(inv: u32, module: usize, a: u32, b: u32) -> u32 {
    *addr_of_mut!(DKRT_TABLE_BAD) = 1;
    dlog::ev(logk::TABLE_BAD, logk::F_ERROR, [inv, module as u32, a, b, 0]);
    inv
}

/// Is the owner byte of a registry or voice row (module record index + 1, 0 = free) a used record?
unsafe fn owner_used(o: u8) -> bool {
    let s = st();
    o == 0 || ((o as usize) <= MAX_MODULES && s.modules[o as usize - 1].used != 0)
}

/// 0 when every invariant holds, else the number of the first one that does not (and one `TABLE_BAD` event).
pub unsafe fn check() -> u32 {
    let s = st();
    let d = dsp::ds();
    let arena_lo = addr_of!(DKRT_ARENA) as usize as u32;
    let area = dsp::area_bytes();
    let mut i = 0;
    // 1. ranges inside the arena and the DSP area
    while i < MAX_MODULES {
        let m = &s.modules[i];
        if m.used != 0 {
            if m.base < arena_lo || m.base - arena_lo > ARENA_BYTES as u32 || m.size > ARENA_BYTES as u32 - (m.base - arena_lo) {
                return fail(1, i, m.base, m.size);
            }
            if d.mod_len[i] != 0 && (d.mod_off[i] > area || d.mod_len[i] > area - d.mod_off[i]) {
                return fail(1, i, d.mod_off[i], d.mod_len[i]);
            }
        }
        i += 1;
    }
    // 2. pairwise disjoint (both ranges are inside their areas now: no overflow)
    i = 0;
    while i < MAX_MODULES {
        if s.modules[i].used != 0 {
            let mut j = i + 1;
            while j < MAX_MODULES {
                if s.modules[j].used != 0 {
                    let (a, b) = (&s.modules[i], &s.modules[j]);
                    if a.base < b.base + b.size && b.base < a.base + a.size {
                        return fail(2, i, a.base, b.base);
                    }
                    if d.mod_len[i] != 0
                        && d.mod_len[j] != 0
                        && d.mod_off[i] < d.mod_off[j] + d.mod_len[j]
                        && d.mod_off[j] < d.mod_off[i] + d.mod_len[i]
                    {
                        return fail(2, i, d.mod_off[i], d.mod_off[j]);
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    // 3. owners and row masks agree
    let mut k = 0;
    while k < dsp::MAX_ROWS {
        let o = d.row_owner[k];
        if o != 0 && o != dkrt_abi::dsp::PLACEHOLDER_OWNER && (o as usize > MAX_MODULES || s.modules[o as usize - 1].used == 0 || d.mod_rows[o as usize - 1] & (1 << k) == 0) {
            return fail(3, if o != 0 && (o as usize) <= MAX_MODULES { o as usize - 1 } else { 0 }, k as u32, o as u32);
        }
        k += 1;
    }
    i = 0;
    while i < MAX_MODULES {
        if s.modules[i].used != 0 {
            k = 0;
            while k < 32 {
                if d.mod_rows[i] & (1 << k) != 0 && (k >= dsp::MAX_ROWS || d.row_owner[k] != i as u8 + 1) {
                    return fail(3, i, k as u32, if k < dsp::MAX_ROWS { d.row_owner[k] as u32 } else { 0xff });
                }
                k += 1;
            }
        } else if d.mod_len[i] != 0 || d.mod_rows[i] != 0 {
            return fail(3, i, d.mod_len[i], d.mod_rows[i]);
        }
        i += 1;
    }
    // the tables of the services (effect rows, ...) against the used records
    let mut used = 0u32;
    i = 0;
    while i < MAX_MODULES {
        if s.modules[i].used != 0 {
            used |= 1 << i;
        }
        i += 1;
    }
    let mut fault = dkrt_abi::service::TableFault::default();
    if crate::handlers::services().table_check(used, &mut fault) != 0 {
        return fail(3, fault.module as usize, fault.a, fault.b);
    }
    // 4. names are unique
    i = 0;
    while i < MAX_MODULES {
        if s.modules[i].used != 0 {
            let mut j = i + 1;
            while j < MAX_MODULES {
                if s.modules[j].used != 0 && cstr_eq(s.modules[i].name.as_ptr(), s.modules[j].name.as_ptr()) {
                    return fail(4, i, j as u32, 0);
                }
                j += 1;
            }
        }
        i += 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lock::LOCK;

    /// A voice row that no module record owns because it is a MISSING placeholder (`PLACEHOLDER_OWNER`) is part of the consistent tables; any other owner of a record
    /// that is not used is invariant 3.
    #[test]
    fn a_placeholder_voice_row_has_no_record_and_is_consistent_and_another_owner_of_a_free_record_is_not() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            *st() = core::mem::zeroed();
            *dsp::ds() = core::mem::zeroed();
            let d = dsp::ds();
            d.row_owner[1] = dkrt_abi::dsp::PLACEHOLDER_OWNER;
            d.row_type[1] = 16;
            assert_eq!(check(), 0);
            d.row_owner[1] = 3; // a record that is not used
            assert_eq!(check(), 3);
            *addr_of_mut!(DKRT_TABLE_BAD) = 0; // a failed check latches the flag for every later test
            d.row_owner[1] = 0;
        }
    }

    /// The core runs the table checks of the loaded services with the mask of the used records (`Service::table_check`): an effect row of a record that is not used, and a
    /// `mod_fx` bit of such a record, are invariant 3 with the numbers the fx service reports; with no service file loaded the effect rows are not the core's to check.
    #[test]
    fn the_core_runs_the_table_checks_of_the_services_and_logs_what_they_report() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let reset = || unsafe {
            *st() = core::mem::zeroed();
            *dsp::ds() = core::mem::zeroed();
            crate::handlers::fx_fixture::reset_rows();
            *addr_of_mut!(DKRT_TABLE_BAD) = 0; // a failed check latches the flag for every later test
        };
        unsafe {
            reset();
            crate::handlers::fx_fixture::frows().fx_owner[1] = 3; // a record that is not used
            assert_eq!(check(), 0, "without the fx service file nothing checks the effect rows");
            let _fx = crate::handlers::fx_fixture::LoadedFx::new();
            assert_eq!(check(), 3);
            assert!(bad());
            reset();
            crate::handlers::fx_fixture::frows().mod_fx[2] = 1; // a row bit of a record that is not used
            assert_eq!(check(), 3);
            reset();
            assert_eq!(check(), 0);
        }
    }
}
