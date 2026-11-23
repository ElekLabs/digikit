//! The voice rows of the DSP's registry: its table, the counts, and the row of a machine type or module.
use crate::rows::{vrows, VoiceRows};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::*;
use dkrt_abi::mem::iw;
use dkrt_sdk::*;

const DYNDSP_NAME: &str = "dyndsp_info";

/// The words of `dyndsp_info` (the DSP's voice registry and its area, from the image's info table); null without it.
pub unsafe fn dyndsp<C: CoreApi>(core: &C) -> *const u32 {
    let t = core.info(dkneeds::keys::name_hash(DYNDSP_NAME));
    if t.is_null() || iw(t, I_MAGIC) != INFO_MAGIC {
        return core::ptr::null();
    }
    t
}

/// (voice rows the area's table reserves, rows in use); (0, 0) without a DSP loader (for CAPACITY).
pub unsafe fn rows_counts<C: CoreApi>(core: &C) -> (u32, u32) {
    let inf = dyndsp(core);
    if inf.is_null() {
        return (0, 0);
    }
    // a placeholder gives way to a real load (`make_room`), so it is no row in use: the planner and `check_rows` count the same
    let used = capacity::used_rows(&vrows().row_owner) - placeholders(vrows());
    (iw(inf, I_VFREE).min(MAX_ROWS as u32), used)
}

/// The voice rows that are MISSING placeholders.
pub fn placeholders(d: &VoiceRows) -> u32 {
    let mut n = 0;
    let mut k = 0;
    while k < MAX_ROWS {
        if d.row_owner[k] == PLACEHOLDER_OWNER {
            n += 1;
        }
        k += 1;
    }
    n
}

/// Whether a voice row (a module's or a placeholder) has machine type `t`.
pub unsafe fn has_voice_row<C: CoreApi>(_core: &C, t: u32) -> bool {
    let d = vrows();
    let mut k = 0;
    while k < MAX_ROWS {
        if d.row_owner[k] != 0 && d.row_type[k] == t {
            return true;
        }
        k += 1;
    }
    false
}

/// Whether a MISSING placeholder row of machine type `t` stands (the row that makes the type silent).
pub unsafe fn has_parked_row<C: CoreApi>(_core: &C, t: u32) -> bool {
    let d = vrows();
    let mut k = 0;
    while k < MAX_ROWS {
        if d.row_owner[k] == PLACEHOLDER_OWNER && d.row_type[k] == t {
            return true;
        }
        k += 1;
    }
    false
}

/// The voice row of machine type `t` that module record `rec` claimed.
pub unsafe fn voice_row<C: CoreApi>(_core: &C, rec: usize, t: u32) -> Option<usize> {
    let d = vrows();
    let mut k = 0;
    while k < MAX_ROWS {
        if d.row_owner[k] == rec as u8 + 1 && d.row_type[k] == t {
            return Some(k);
        }
        k += 1;
    }
    None
}

/// What `init` declared for the voice row `row` (a module): the eight knob ranges and the latch mask, published with the row. A latch mask needs a
/// note_on routine (as the DSP section check says for version 3). Returns OK or E_FORMAT.
pub unsafe fn set_declared<C: CoreApi>(_core: &C, row: usize, rng: [u32; 8], latched: u32) -> i32 {
    let d = vrows();
    if row >= MAX_ROWS || (latched & !0xff != 0) || (latched != 0 && d.row_note_on[row] == 0) {
        return E_FORMAT;
    }
    d.row_rng[row] = rng;
    d.row_latched[row] = latched;
    OK
}

/// The module record (index) that owns machine type `t`.
pub unsafe fn module_of_type<C: CoreApi>(_core: &C, t: u32) -> Option<usize> {
    let d = vrows();
    let mut k = 0;
    while k < MAX_ROWS {
        if d.row_owner[k] != 0 && d.row_owner[k] != PLACEHOLDER_OWNER && d.row_type[k] == t {
            return Some(d.row_owner[k] as usize - 1);
        }
        k += 1;
    }
    None
}
