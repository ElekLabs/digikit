//! Voice-row part of the DSP loader: the voice row of a machine type, its declared ranges and the clearing of a module's voice rows. The rows are the service's own
//! (`rows::VoiceRows`); the table of the DSP's voice registry is read through `CoreApi::info` (`dyndsp`).
use crate::rows::{vrows, VoiceRows};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::*;
use dkrt_abi::link::{Owner, Sequence};
use dkrt_abi::mem::{iw, rd32};
use dkrt_abi::service::TableFault;
use dkrt_abi::service::{Claim, RowWrite};
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

/// Row `k` of the voice table, as the service keeps it, to `owner`, `ty` and `pc` with every other field cleared (a free row, or a placeholder).
#[inline(never)]
unsafe fn reset_row(d: &mut VoiceRows, k: usize, owner: u8, ty: u32, pc: u32) {
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
    for i in 0..job.nfilter {
        let key = rd32(job.filters.add((i * dkneeds::filter::RECORD) as usize));
        let ty = crate::filter::resolve(core, key);
        if !(crate::filter::FIRST..=crate::filter::LAST).contains(&ty) {
            return E_DSP_TYPE;
        }
        let owner = filter_rows::row((ty - crate::filter::FIRST) as usize).owner;
        if owner != 0 && owner != core.live_op() {
            return E_DSP_TYPE;
        }
    }
    job.accepted += nvoice + job.nfilter;
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
    filter_rows::claim(core, c);
    d.mod_rows[rec] = claimed;
}

/// Give back the voice rows module record `rec` claimed.
pub(crate) unsafe fn release<C: CoreApi>(core: &C, rec: usize) {
    filter_rows::release(rec);
    let d = vrows();
    let mut k = 0;
    while k < MAX_ROWS {
        if d.row_owner[k] == rec as u8 + 1 {
            reset_row(d, k, 0, FREE_TYPE, 0);
        }
        k += 1;
    }
    d.mod_rows[rec] = 0;
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

// ---- the check of the table ------------------------------------------------------------------------------------------------

/// The voice table against the module records (`Service::table_check`): an owned row belongs to a used record whose `mod_rows` has its bit (a MISSING placeholder,
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

pub mod filter_rows;
