//! `Service::dsp_admit`: a real load always wins a voice row.
use super::*;
use crate::rows::vrows;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::*;
use dkrt_abi::link::{Owner, Sequence};
use dkrt_abi::mem::{iw, rd32};
use dkrt_sdk::*;

/// `Service::dsp_admit`: a real load always wins a voice row. When fewer free rows exist than voices that have no placeholder or own row of their type, the placeholders of
/// other numbers are freed (the type word first, then the disabled word) until the rows are there, and `gave` is told each type: the caller turns its machine row into a gap
/// row and logs it. `E_BUSY` when the DSP did not take a write (nothing is reserved yet; the load is tried again).
pub(crate) unsafe fn make_room<C: CoreApi>(core: &C, job: &Job, gave: &mut dyn FnMut(u32)) -> i32 {
    if dyndsp(core).is_null() {
        return OK;
    }
    let d = vrows();
    let types_of_job = |t: u32| {
        let mut i = 0;
        while i < job.nvoice as usize {
            let e = job.voices.add(job.vstride as usize * i);
            if crate::assign::resolve(core, rd32(e.add(VOICE_TYPE as usize))) == t {
                return true;
            }
            i += 1;
        }
        false
    };
    let mut need = 0;
    let mut i = 0;
    while i < job.nvoice as usize {
        let e = job.voices.add(job.vstride as usize * i);
        let t = crate::assign::resolve(core, rd32(e.add(VOICE_TYPE as usize)));
        let mut have = false;
        let mut k = 0;
        while k < MAX_ROWS {
            if (d.row_owner[k] == PLACEHOLDER_OWNER
                || (d.row_owner[k] as u32 == core.live_op() && d.row_owner[k] != 0))
                && d.row_type[k] == t
            {
                have = true;
            }
            k += 1;
        }
        if !have {
            need += 1;
        }
        i += 1;
    }
    let first = iw(dyndsp(core), I_VFIRST);
    let mut free = capacity::free_rows(&d.row_owner, iw(dyndsp(core), I_VFREE));
    while free < need {
        let mut pick = MAX_ROWS;
        let mut k = 0;
        while k < MAX_ROWS {
            if d.row_owner[k] == PLACEHOLDER_OWNER && !types_of_job(d.row_type[k]) {
                pick = k;
            }
            k += 1;
        }
        if pick == MAX_ROWS {
            break; // `check_rows` counted exactly these rows: nothing more to give
        }
        let at = ROW_WORDS * (first + pick as u32);
        let mut seq = Sequence::new();
        seq.push(WIN_VOICES, at, FREE_TYPE);
        seq.push(WIN_VOICES, at + 10, 0);
        if !core.link_run(Owner::Core, &seq) {
            return E_BUSY;
        }
        let t = d.row_type[pick];
        reset_row(d, pick, 0, FREE_TYPE, 0);
        gave(t);
        free += 1;
    }
    OK
}
