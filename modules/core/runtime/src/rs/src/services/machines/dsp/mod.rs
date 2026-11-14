//! Voice-row part of the DSP loader: the voice row of a machine type, its declared ranges and the clearing of a module's voice rows.
use crate::dsp::*;
use dkrt_sdk::*;

/// The voice row of machine type `t` that module record `rec` claimed.
pub unsafe fn voice_row(rec: usize, t: u32) -> Option<usize> {
    let d = ds();
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
pub unsafe fn set_declared(row: usize, rng: [u32; 8], latched: u32) -> i32 {
    let d = ds();
    if row >= MAX_ROWS || (latched & !0xff != 0) || (latched != 0 && d.row_note_on[row] == 0) {
        return E_FORMAT;
    }
    d.row_rng[row] = rng;
    d.row_latched[row] = latched;
    OK
}

/// The module record (index) that owns machine type `t`.
pub unsafe fn module_of_type(t: u32) -> Option<usize> {
    let d = ds();
    let mut k = 0;
    while k < MAX_ROWS {
        if d.row_owner[k] != 0 && d.row_type[k] == t {
            return Some(d.row_owner[k] as usize - 1);
        }
        k += 1;
    }
    None
}

/// Free the rows module record `rec` published: the type word first, so the scan skips the row at once.
pub(crate) unsafe fn clear_rows(rec: usize) -> bool {
    let d = ds();
    let first = iw(I_VFIRST);
    let mut k = 0;
    while k < MAX_ROWS {
        if d.row_owner[k] == rec as u8 + 1 && d.row_pub[k] != 0 {
            if !write_word_blocking(WIN_VOICES, ROW_WORDS * (first + k as u32), FREE_TYPE) { return false; }
            d.row_pub[k] = 0;
        }
        k += 1;
    }
    true
}
