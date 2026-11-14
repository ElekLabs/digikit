//! Voice-row part of the DSP loader: the voice row of a machine type, its declared ranges and the clearing of a module's voice rows.
use crate::dsp::*;
use crate::handlers::{Claim, RowWrite};
use crate::rd32;
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

/// DSP section, version 3 voices: the state counts must be sane before the range is computed from them.
pub(crate) unsafe fn check_sections(sec: &DspSec) -> i32 {
    let (s, voice_off, vstride, nvoice, image_size) = (sec.s, sec.voice_off, sec.vstride, sec.nvoice, sec.image_size);
    if vstride == VOICE_V3 {
        let mut i = 0;
        while i < nvoice {
            let e = s.add((voice_off + vstride * i) as usize);
            let (note_on, state, clear, count) = (rd32(e.add(8)), rd32(e.add(12)), rd32(e.add(16)), rd32(e.add(52)));
            if note_on >= image_size || note_on & 1 != 0 || clear > state || state > iw(I_AREA_BYTES) || count > MAX_STATE_VOICES {
                return E_DSP_FORMAT;
            }
            let latched = rd32(e.add(56));
            if latched & !0xff != 0 || (latched != 0 && note_on == 0) || rd32(e.add(60)) != 0 {
                return E_DSP_FORMAT;
            }
            i += 1;
        }
    }
    OK
}

/// DSP section: voice rows are free for the module's voices, their types are in the run-time range, their entries are inside the
/// image, and no loaded or loading module or earlier voice of this file has the type.
pub(crate) unsafe fn check_rows(sec: &DspSec) -> i32 {
    let (s, voice_off, vstride, nvoice, image_size) = (sec.s, sec.voice_off, sec.vstride, sec.nvoice, sec.image_size);
    let d = ds();
    let r = capacity::dsp_rows(nvoice, capacity::free_rows(&d.row_owner, iw(I_VFREE)));
    if r != OK {
        return r;
    }
    let mut i = 0;
    while i < nvoice {
        let e = s.add(voice_off as usize + vstride as usize * i as usize);
        // the voice names its machine by key (or by a pinned number): `assign` has the type
        let t = crate::assign::resolve(rd32(e));
        let entry = rd32(e.add(4));
        if !(TYPE_FIRST..=TYPE_LAST).contains(&t) || entry >= image_size || entry & 1 != 0 {
            return E_DSP_TYPE;
        }
        let mut k = 0;
        while k < MAX_ROWS {
            if d.row_owner[k] != 0 && d.row_type[k] == t {
                return E_DSP_TYPE; // used by a module that is loaded or loading
            }
            k += 1;
        }
        let mut j = 0;
        while j < i {
            if crate::assign::resolve(rd32(s.add(voice_off as usize + vstride as usize * j as usize))) == t {
                return E_DSP_TYPE; // twice in this file
            }
            j += 1;
        }
        i += 1;
    }
    OK
}

/// Claim the voice rows (lowest free ones first): the state blocks follow image and bss (8-aligned), one block of `voices * stride`
/// per voice record, in record order.
pub(crate) unsafe fn claim(c: &mut Claim) {
    let (rec, job, base, byte_shift) = (c.rec, c.job, c.base, c.byte_shift);
    let d = ds();
    let mut state_at = c.state_at;
    let mut claimed = 0u32;
    let mut v = 0;
    let mut row = 0;
    while v < job.nvoice as usize {
        while row < MAX_ROWS && (d.row_owner[row] != 0 || (row as u32) >= iw(I_VFREE)) {
            row += 1;
        }
        let e = job.voices.add(job.vstride as usize * v);
        d.row_type[row] = crate::assign::resolve(rd32(e));
        d.row_pc[row] = L2_SW_BASE + (base + rd32(e.add(4)) - L2_BYTE_BASE) / 2;
        let mut k = 0;
        while k < 8 {
            d.row_rng[row][k] = match job.vstride {
                VOICE_V2 => rd32(e.add(8 + 4 * k)),
                VOICE_V3 => rd32(e.add(20 + 4 * k)),
                _ => 0,
            };
            k += 1;
        }
        if job.vstride == VOICE_V3 {
            let note_on = rd32(e.add(8));
            let stride = (rd32(e.add(12)) + 7) & !7;
            d.row_note_on[row] = if note_on == 0 { 0 } else { L2_SW_BASE + (base + note_on - L2_BYTE_BASE) / 2 };
            // the byte address of a data symbol at image offset `state_at`: nominal L2_BYTE_BASE + state_at, plus the byte shift
            d.row_state_base[row] = L2_BYTE_BASE + state_at + byte_shift;
            d.row_stride[row] = stride;
            d.row_clear[row] = rd32(e.add(16));
            d.row_latched[row] = rd32(e.add(56));
            state_at += rd32(e.add(52)) * stride;
        } else {
            d.row_note_on[row] = 0;
            d.row_state_base[row] = 0;
            d.row_stride[row] = 0;
            d.row_clear[row] = 0;
            d.row_latched[row] = 0;
        }
        d.row_owner[row] = rec as u8 + 1;
        d.row_pub[row] = 0;
        claimed |= 1 << row;
        row += 1;
        v += 1;
    }
    c.state_at = state_at;
    d.mod_rows[rec] = claimed;
}

/// Give back the voice rows module record `rec` claimed.
pub(crate) unsafe fn release(rec: usize) {
    let d = ds();
    let mut k = 0;
    while k < MAX_ROWS {
        if d.row_owner[k] == rec as u8 + 1 {
            d.row_owner[k] = 0;
            d.row_type[k] = FREE_TYPE;
            d.row_pub[k] = 0;
            d.row_pc[k] = 0;
            d.row_rng[k] = [0; 8];
            d.row_note_on[k] = 0;
            d.row_state_base[k] = 0;
            d.row_stride[k] = 0;
            d.row_clear[k] = 0;
            d.row_latched[k] = 0;
        }
        k += 1;
    }
    d.mod_rows[rec] = 0;
}

/// The voice-row word to publish at step `*want` of module `rec`: a row takes `ROW_STEPS` steps (the 8 ranges, the pc, note_on, state
/// base, stride, clear bytes, the latch mask, the zeroed-tracks mask, the disabled word cleared, the type).
pub(crate) unsafe fn row_step(rec: usize, want: &mut u32, write: bool) -> Option<RowWrite> {
    let d = ds();
    let mut row = MAX_ROWS;
    let mut k = 0;
    while k < MAX_ROWS {
        if d.row_owner[k] == rec as u8 + 1 {
            if *want < ROW_STEPS {
                row = k;
                break;
            }
            *want -= ROW_STEPS;
        }
        k += 1;
    }
    if row == MAX_ROWS {
        return None;
    }
    if !write {
        return Some(RowWrite { win: 0, word: 0, value: 0, published: None });
    }
    let first = iw(I_VFIRST);
    let want = *want;
    let at = ROW_WORDS * (first + row as u32);
    let (word, value) = if want < 8 {
        (at + 2 + want, d.row_rng[row][want as usize])
    } else if want == 8 {
        (at + 1, d.row_pc[row])
    } else if want == 9 {
        (at + 16, d.row_note_on[row])
    } else if want == 10 {
        (at + 17, d.row_state_base[row])
    } else if want == 11 {
        (at + 18, d.row_stride[row])
    } else if want == 12 {
        (at + 19, d.row_clear[row])
    } else if want == 13 {
        (at + 22, d.row_latched[row])
    } else if want == 14 {
        (at + 23, 0) // no track has had its state zeroed since this load (the adapter zeroes it at the track's first note-on)
    } else if want == 15 {
        (at + 10, 0)
    } else {
        (at, d.row_type[row])
    };
    // from the last step on, the type word may be in the DSP: clear it on failure
    let published = if want == ROW_STEPS - 1 { Some(&mut d.row_pub[row]) } else { None };
    Some(RowWrite { win: WIN_VOICES, word, value, published })
}
