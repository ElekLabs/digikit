//! Claiming and giving back the voice rows of a module, and the row words in publish order.
use super::*;
use crate::rows::vrows;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::*;
use dkrt_abi::mem::{iw, rd32};
use dkrt_abi::service::{Claim, RowWrite};

/// Claim the voice rows (lowest free ones first): the state blocks follow image and bss (8-aligned), one block of `voices * stride`
/// per voice record, in record order.
pub(crate) unsafe fn claim<C: CoreApi>(core: &C, c: &mut Claim) {
    let (rec, job, base, byte_shift) = (c.rec, c.job, c.base, c.byte_shift);
    let d = vrows();
    let mut state_at = c.state_at;
    let mut claimed = 0u32;
    let mut v = 0;
    let mut row = 0;
    while v < job.nvoice as usize {
        while row < MAX_ROWS && (d.row_owner[row] != 0 || (row as u32) >= iw(dyndsp(core), I_VFREE))
        {
            row += 1;
        }
        let e = job.voices.add(job.vstride as usize * v);
        let t = crate::assign::resolve(core, rd32(e.add(VOICE_TYPE as usize)));
        // the placeholder row of this type, if a module of the same number was unloaded: its disabled word keeps the type silent until the row is published
        let mut parked = MAX_ROWS;
        let mut p = 0;
        while p < MAX_ROWS {
            if d.row_owner[p] == PLACEHOLDER_OWNER && d.row_type[p] == t {
                parked = p;
            }
            p += 1;
        }
        let fresh = parked == MAX_ROWS;
        let at = if fresh { row } else { parked };
        d.row_type[at] = t;
        d.row_pc[at] = L2_SW_BASE + (base + rd32(e.add(VOICE_ENTRY as usize)) - L2_BYTE_BASE) / 2;
        let mut k = 0;
        while k < 8 {
            d.row_rng[at][k] = match job.vstride {
                VOICE_V2 => rd32(e.add(VOICE_V2_RANGES as usize + 4 * k)),
                VOICE_V3 => rd32(e.add(VOICE_V3_RANGES as usize + 4 * k)),
                _ => 0,
            };
            k += 1;
        }
        if job.vstride == VOICE_V3 {
            let note_on = rd32(e.add(VOICE_NOTE_ON as usize));
            let stride = (rd32(e.add(VOICE_STATE_BYTES as usize)) + 7) & !7;
            d.row_note_on[at] = if note_on == 0 {
                0
            } else {
                L2_SW_BASE + (base + note_on - L2_BYTE_BASE) / 2
            };
            // the byte address of a data symbol at image offset `state_at`: nominal L2_BYTE_BASE + state_at, plus the byte shift
            d.row_state_base[at] = L2_BYTE_BASE + state_at + byte_shift;
            d.row_stride[at] = stride;
            d.row_clear[at] = rd32(e.add(VOICE_CLEAR_BYTES as usize));
            d.row_latched[at] = rd32(e.add(VOICE_LATCH as usize));
            state_at += rd32(e.add(VOICE_VOICES as usize)) * stride;
        } else {
            d.row_note_on[at] = 0;
            d.row_state_base[at] = 0;
            d.row_stride[at] = 0;
            d.row_clear[at] = 0;
            d.row_latched[at] = 0;
        }
        d.row_owner[at] = rec as u8 + 1;
        // 2: the DSP holds a disabled placeholder of this type (`row_step` marks the row live one step earlier)
        d.row_pub[at] = if fresh { 0 } else { 2 };
        claimed |= 1 << at;
        if fresh {
            row += 1;
        }
        v += 1;
    }
    c.state_at = state_at;
    d.mod_rows[rec] = claimed;
    crate::modules::publish(core);
}

/// Give back the voice rows module record `rec` claimed.
pub(crate) unsafe fn release<C: CoreApi>(core: &C, rec: usize) {
    let d = vrows();
    let mut k = 0;
    while k < MAX_ROWS {
        if d.row_owner[k] == rec as u8 + 1 {
            reset_row(d, k, 0, FREE_TYPE, 0);
        }
        k += 1;
    }
    d.mod_rows[rec] = 0;
    crate::modules::publish(core);
}

/// The voice-row word to publish at step `*want` of module `rec`: a row takes `ROW_STEPS` steps (the 8 ranges, the pc, note_on, state
/// base, stride, clear bytes, the latch mask, the zeroed-tracks mask, the disabled word cleared, the type).
pub(crate) unsafe fn row_step<C: CoreApi>(
    core: &C,
    rec: usize,
    want: &mut u32,
    write: bool,
) -> Option<RowWrite> {
    let d = vrows();
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
        return Some(RowWrite {
            win: 0,
            word: 0,
            value: 0,
            published: None,
            last: false,
            has_undo: false,
            undo: 0,
        });
    }
    let first = iw(dyndsp(core), I_VFIRST);
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
    // A fresh row goes live at the type word (the last step): from then on the DSP may hold it, so a failure frees it again (`undo`, and `row_pub` marks it
    // for `clear_rows`). A row that reuses a placeholder (`claim` set `row_pub` to 2) is live, disabled, with its type word in place: it goes live at the word-10
    // clear, one step before the type write, so that step carries the mark and the undo (the disabled word takes the type again), and the type write, which
    // changes nothing, has neither.
    let reused = d.row_pub[row] == 2;
    let live_step = if reused { ROW_STEPS - 2 } else { ROW_STEPS - 1 };
    let published = if want == live_step {
        Some(&mut d.row_pub[row])
    } else {
        None
    };
    let last = want == ROW_STEPS - 1;
    let has_undo = want == live_step;
    let undo = if !has_undo {
        0
    } else if reused {
        d.row_type[row]
    } else {
        FREE_TYPE
    };
    Some(RowWrite {
        win: WIN_VOICES,
        word,
        value,
        published,
        last,
        has_undo,
        undo,
    })
}
