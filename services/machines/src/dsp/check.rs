//! The checks of a DSP section and of the voice table against the module records.
use super::*;
use crate::rows::vrows;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::*;
use dkrt_abi::mem::{iw, rd32};
use dkrt_abi::service::TableFault;
use dkrt_sdk::*;

/// DSP section, the voice table: its counts and entries are inside the section, and (version 3) the state counts are sane before the range is computed from them. Each
/// voice's state block (`voices * state_bytes`, rounded to 8 bytes) is added to `job.state_bytes`.
pub(crate) unsafe fn check_sections<C: CoreApi>(core: &C, sec: &DspSec, job: &mut Job) -> i32 {
    let (s, voice_off, vstride, nvoice, image_size) = (
        sec.s,
        sec.voice_off,
        sec.vstride,
        sec.nvoice,
        sec.image_size,
    );
    if nvoice as usize > MAX_VOICES || !in_sec(voice_off, nvoice * vstride, sec.size) {
        return E_DSP_FORMAT;
    }
    if vstride == VOICE_V3 {
        let mut i = 0;
        while i < nvoice {
            let e = s.add((voice_off + vstride * i) as usize);
            let (note_on, state, clear, count) = (
                rd32(e.add(VOICE_NOTE_ON as usize)),
                rd32(e.add(VOICE_STATE_BYTES as usize)),
                rd32(e.add(VOICE_CLEAR_BYTES as usize)),
                rd32(e.add(VOICE_VOICES as usize)),
            );
            if note_on >= image_size
                || note_on & 1 != 0
                || clear > state
                || state > iw(dyndsp(core), I_AREA_BYTES)
                || count > MAX_STATE_VOICES
            {
                return E_DSP_FORMAT;
            }
            let latched = rd32(e.add(VOICE_LATCH as usize));
            if latched & !0xff != 0
                || (latched != 0 && note_on == 0)
                || rd32(e.add(VOICE_V3_RESERVED as usize)) != 0
            {
                return E_DSP_FORMAT;
            }
            job.state_bytes += count * ((state + 7) & !7);
            i += 1;
        }
    }
    job.accepted += nvoice;
    OK
}

/// DSP section: voice rows are free for the module's voices. A placeholder is free for any real load (`make_room` gives it back first); a row of the module being reloaded is
/// free for a voice of its type (the teardown that comes before the load parks it).
pub(crate) unsafe fn check_rows<C: CoreApi>(core: &C, sec: &DspSec) -> i32 {
    let nvoice = sec.nvoice;
    let d = vrows();
    let mut own = 0;
    let mut i = 0;
    while i < nvoice {
        let e = sec
            .s
            .add(sec.voice_off as usize + sec.vstride as usize * i as usize);
        let t = crate::assign::resolve(core, rd32(e.add(VOICE_TYPE as usize)));
        let mut k = 0;
        while k < MAX_ROWS {
            if d.row_owner[k] as u32 == core.live_op() && d.row_owner[k] != 0 && d.row_type[k] == t
            {
                own += 1;
                break;
            }
            k += 1;
        }
        i += 1;
    }
    capacity::dsp_rows(
        nvoice,
        capacity::free_rows(&d.row_owner, iw(dyndsp(core), I_VFREE)) + placeholders(d) + own,
    )
}

/// DSP section: the voices' types are in the run-time range, their entries are inside the image, and no loaded or loading module or
/// earlier voice of this file has the type.
pub(crate) unsafe fn check_types<C: CoreApi>(core: &C, sec: &DspSec) -> i32 {
    let (s, voice_off, vstride, nvoice, image_size) = (
        sec.s,
        sec.voice_off,
        sec.vstride,
        sec.nvoice,
        sec.image_size,
    );
    let d = vrows();
    let mut i = 0;
    while i < nvoice {
        let e = s.add(voice_off as usize + vstride as usize * i as usize);
        // the voice names its machine by key (or by a pinned number): `assign` has the type
        let t = crate::assign::resolve(core, rd32(e.add(VOICE_TYPE as usize)));
        let entry = rd32(e.add(VOICE_ENTRY as usize));
        if !crate::profile(core).is_some_and(|p| p.is_dyn_type(t))
            || entry >= image_size
            || entry & 1 != 0
        {
            return E_DSP_TYPE;
        }
        let mut k = 0;
        while k < MAX_ROWS {
            // (a placeholder row keeps the number of an unloaded module: the module that loads again may take it)
            if d.row_owner[k] != 0
                && d.row_owner[k] != PLACEHOLDER_OWNER
                && d.row_owner[k] as u32 != core.live_op()
                && d.row_type[k] == t
            {
                return E_DSP_TYPE; // used by a module that is loaded or loading (the module being reloaded does not count: it goes first)
            }
            k += 1;
        }
        let mut j = 0;
        while j < i {
            if crate::assign::resolve(
                core,
                rd32(
                    s.add(voice_off as usize + vstride as usize * j as usize + VOICE_TYPE as usize),
                ),
            ) == t
            {
                return E_DSP_TYPE; // twice in this file
            }
            j += 1;
        }
        i += 1;
    }
    OK
}

// ---- the check of the table ------------------------------------------------------------------------------------------------

/// The voice table against the module records (`Describe::table_check`): an owned row belongs to a used record whose `mod_rows` has its bit (a MISSING placeholder,
/// `PLACEHOLDER_OWNER`, belongs to none), and every bit of `mod_rows` is a row the record owns and the record is used. The mismatch is in `out` as (record, row, the
/// row's owner).
pub(crate) unsafe fn table_check(used: u32, out: &mut TableFault) -> u32 {
    let r = vrows();
    let mut k = 0;
    while k < MAX_ROWS {
        let o = r.row_owner[k];
        if o != 0
            && o != PLACEHOLDER_OWNER
            && (o as usize > dkrt_abi::MAX_MODULES
                || used & (1 << (o - 1)) == 0
                || r.mod_rows[o as usize - 1] & (1 << k) == 0)
        {
            let m = if (o as usize) <= dkrt_abi::MAX_MODULES {
                o as u32 - 1
            } else {
                0
            };
            *out = TableFault {
                module: m,
                a: k as u32,
                b: o as u32,
            };
            return 1;
        }
        k += 1;
    }
    let mut i = 0;
    while i < dkrt_abi::MAX_MODULES {
        k = 0;
        while k < 32 {
            if r.mod_rows[i] & (1 << k) != 0
                && (k >= MAX_ROWS || used & (1 << i) == 0 || r.row_owner[k] != i as u8 + 1)
            {
                let o = if k < MAX_ROWS {
                    r.row_owner[k] as u32
                } else {
                    0xff
                };
                *out = TableFault {
                    module: i as u32,
                    a: k as u32,
                    b: o,
                };
                return 2;
            }
            k += 1;
        }
        i += 1;
    }
    0
}
