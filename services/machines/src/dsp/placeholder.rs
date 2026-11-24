//! The voice rows a module leaves: clearing them, parking a placeholder for an absent module and freeing placeholders.
use super::*;
use crate::rows::{vrows, VoiceRows};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::*;
use dkrt_abi::link::{Owner, Sequence};
use dkrt_abi::mem::iw;

/// Row `k` of the voice table, as the service keeps it, to `owner`, `ty` and `pc` with every other field cleared (a free row, or a placeholder).
#[inline(never)]
pub(super) unsafe fn reset_row(d: &mut VoiceRows, k: usize, owner: u8, ty: u32, pc: u32) {
    d.row_owner[k] = owner;
    d.row_type[k] = ty;
    d.row_pub[k] = 0;
    d.row_pc[k] = pc;
    d.row_rng[k] = [0; 8];
    d.row_note_on[k] = 0;
    d.row_state_base[k] = 0;
    d.row_stride[k] = 0;
    d.row_clear[k] = 0;
    d.row_latched[k] = 0;
}

/// Take the rows module record `rec` published out of the DSP, so the module's code can go. A row whose number stays MISSING (the module is reloading, or the sticky map
/// holds the number: `assign::holds`) becomes a placeholder: the disabled word (word 10) takes the row's type and the pc a harmless value, then the row belongs to no
/// record (`PLACEHOLDER_OWNER`). The dispatch finds the type and renders a silent block for it, where a freed row would send the type to the first stock machine's sound
/// (docs/plans/extension-model.md E5 item 7, E5d). The next module that claims a voice of the same type takes the placeholder row itself (`claim`) and publishes its words
/// over the disabled one. Any other row is freed (the type word first, so the scan skips the row at once). `free_placeholders` frees a placeholder when its number leaves
/// the map. A row that was claimed but never published is freed with the module (`release`). False when a write failed: the caller keeps everything.
pub(crate) unsafe fn clear_rows<C: CoreApi>(core: &C, rec: usize) -> bool {
    let d = vrows();
    let first = iw(dyndsp(core), I_VFIRST);
    let reload = core.live_op() != 0;
    let mut k = 0;
    while k < MAX_ROWS {
        if d.row_owner[k] == rec as u8 + 1 && d.row_pub[k] != 0 {
            let park = reload || crate::assign::holds(d.row_type[k]);
            let at = ROW_WORDS * (first + k as u32);
            let mut seq = Sequence::new();
            if park {
                seq.push(WIN_VOICES, at + 10, d.row_type[k]);
                seq.push(WIN_VOICES, at + 1, PLACEHOLDER_PC);
            } else {
                seq.push(WIN_VOICES, at, FREE_TYPE);
            }
            if !core.link_run(Owner::Core, &seq) {
                crate::modules::publish(core);
                return false;
            }
            if park {
                // the row is invalidated (disabled): `row_pub` stands for the module's own row
                reset_row(d, k, PLACEHOLDER_OWNER, d.row_type[k], PLACEHOLDER_PC);
                d.mod_rows[rec] &= !(1 << k);
            } else {
                d.row_pub[k] = 0;
            }
        }
        k += 1;
    }
    crate::modules::publish(core);
    true
}

/// Park a voice row for machine type `t` whose module is absent and that has no voice row yet (a boot where the module is not loaded, a module that failed before its row
/// was published): the disabled word, the pc, then the type word, so the row is never live without its disabled word. `Some(true)` when the type has a row (or needs none: a
/// module owns it, there is no DSP loader), `Some(false)` when no row is free (the budget: a placeholder takes a row for each absent number), `None` when the DSP did not
/// take the write (the caller tries again later). The write waits on the link: the caller runs it when the loader is idle.
pub(crate) unsafe fn park_missing<C: CoreApi>(core: &C, t: u32) -> Option<bool> {
    if dyndsp(core).is_null() {
        return Some(true);
    }
    let d = vrows();
    let mut free = MAX_ROWS;
    let mut k = 0;
    while k < MAX_ROWS {
        if d.row_owner[k] != 0 && d.row_type[k] == t {
            return Some(true);
        }
        if free == MAX_ROWS && d.row_owner[k] == 0 && (k as u32) < iw(dyndsp(core), I_VFREE) {
            free = k;
        }
        k += 1;
    }
    if free == MAX_ROWS {
        return Some(false);
    }
    let first = iw(dyndsp(core), I_VFIRST);
    let at = ROW_WORDS * (first + free as u32);
    let mut seq = Sequence::new();
    seq.push(WIN_VOICES, at + 10, t);
    seq.push(WIN_VOICES, at + 1, PLACEHOLDER_PC);
    seq.push(WIN_VOICES, at, t);
    if !core.link_run(Owner::Core, &seq) {
        return None;
    }
    reset_row(d, free, PLACEHOLDER_OWNER, t, PLACEHOLDER_PC);
    Some(true)
}

/// Whether the DSP loader has nothing in flight, so a placeholder write does not compete with a module's rows.
pub(crate) unsafe fn loader_idle<C: CoreApi>(core: &C) -> bool {
    let d = core.dsp();
    d.phase == PH_IDLE && d.nwait == 0
}

/// Free every placeholder row whose number the sticky map no longer holds (`assign::holds`): a reload that ended without the new version keeps the MISSING placeholder of a
/// number the map still holds, and an eviction that gave the number to another module is taken over at its claim. The type word first, then the disabled word; a failed write
/// leaves the row for the next end of a reload.
pub(crate) unsafe fn free_placeholders<C: CoreApi>(core: &C) {
    if dyndsp(core).is_null() {
        return; // an image without the DSP loader has no voice rows
    }
    let d = vrows();
    let first = iw(dyndsp(core), I_VFIRST);
    let mut k = 0;
    while k < MAX_ROWS {
        if d.row_owner[k] == PLACEHOLDER_OWNER && !crate::assign::holds(d.row_type[k]) {
            let at = ROW_WORDS * (first + k as u32);
            let mut seq = Sequence::new();
            seq.push(WIN_VOICES, at, FREE_TYPE);
            seq.push(WIN_VOICES, at + 10, 0);
            if core.link_run(Owner::Core, &seq) {
                reset_row(d, k, 0, FREE_TYPE, 0);
            }
        }
        k += 1;
    }
}
