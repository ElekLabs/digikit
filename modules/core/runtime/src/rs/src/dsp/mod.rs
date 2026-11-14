//! The DSP side of run-time modules (docs/design/digikit-dkm.md "DSP section", docs/findings/32-runtime-dsp-machines.md).
//!
//! A `.dkm` may carry SHARC+ code and data for the DSP. The runtime places it in a range of the DSP area (the `_dl_area` of
//! `core/dsp-link`, first fit, freed when the DSP part of a boot-time load fails), applies the relocations on the ColdFire, sends the bytes with `dl_send` and
//! publishes the module's voice rows. The transfer takes many audio blocks, so a load is a state machine that the UI-loop hook
//! advances (`step`); `dkrt_load` returns `LOADING` and the machine is listed only when the DSP code is acknowledged
//! and the voice rows are in.
//!
//! Voice rows (the voices registry of `core/dsp-voice`, scanned by the DSP for every voice it renders) are 24 words
//! (`ROW_WORDS`) and written one word per transfer through window 1 of `core/dsp-link`: the eight knob ranges (words 2 to 9: the
//! runtime guard clamps the machine's parameter words to them), the pc, the note_on pc (16), the state base (17), the state stride
//! (18) and the clear bytes (19), the latch mask (22), the zeroed-tracks mask (23) cleared, the guard's disabled-type word cleared, and the type
//! last (the row is still free until then: its type is `0xffffffff`, which no machine type equals), so the render scan never sees a
//! half-written row. A row is freed the other way round (type to `0xffffffff` first). The generation (20) and log level (21) words
//! are not written yet (0).
//!
//! Effect rows (DSP section version 4, `core/dynfx`): the rows of the effect chain of `core/fx` (24 words each, window 2 of `core/dsp-link`). An effect
//! record claims a free row (pc `0xffffffff`) when the range is reserved and publishes it after the code is acknowledged, one word per transfer: the
//! eight parameter words (the record's defaults), desc, state, `enabled` (1) and the pc last, so the chain never sees a half-written row. A row is freed
//! the other way round (the pc to `0xffffffff` first, then the adapter's `was`, `dis` and fault counters cleared). The effect's state is reserved in the
//! module's range behind the image, bss and voice states (`state_bytes` of its `dk_fx_desc`).
//!
//! Nothing here runs unless the file has a DSP section; the link to `core/dsp-link`, `core/dyndsp` and `core/dynfx` is optional.
use crate::handlers::{Claim, ServiceList, Services};
use crate::{arena, cstr_eq, cstr_len, rd32};
use core::ptr::{addr_of_mut, read_volatile};
use dkrt_sdk::*;

pub(crate) use dkrt_abi::dsp::*;

pub unsafe fn retained() -> bool { ds().phase == PH_RETAINED }


extern "C" {
    // Symbols of core/dsp-link and core/dyndsp; each resolves to 0 when the module is not in the image (`[object] optional`).
    fn dl_send();
    fn dl_send_win();
    fn dl_poll();
    fn dyndsp_info();
    // core/dynfx: the effect chain's table; 0 when the module is not in the image.
    fn dynfx_info();
}

#[repr(C)]
pub struct DlStatus {
    pub state: u32,
    pub err: u32,
    pub nchunks: u32,
    pub acked: u32,
    pub sent: u32,
    pub resent: u32,
    pub nerr: u32,
}
pub(crate) const DL_DONE: u32 = 2;
pub(crate) const DL_ERROR: u32 = 3;

#[no_mangle]
pub static mut DKRT_DSP: DspState = unsafe { core::mem::zeroed() };

pub fn ds() -> &'static mut DspState {
    unsafe { &mut *addr_of_mut!(DKRT_DSP) }
}

pub(crate) fn fn_addr(f: unsafe extern "C" fn()) -> usize {
    core::hint::black_box(f as usize)
}

/// The info table of `core/dyndsp`, or null.
pub(crate) unsafe fn info() -> *const u32 {
    #[cfg(test)] { return crate::cleanup_tests::voice_info(); }
    #[cfg(not(test))] {
        let a = fn_addr(dyndsp_info);
        if a == 0 {
            return core::ptr::null();
        }
        let p = a as *const u32;
        if read_volatile(p.add(I_MAGIC)) != INFO_MAGIC {
            return core::ptr::null();
        }
        p
    }
}

pub(crate) unsafe fn iw(k: usize) -> u32 {
    read_volatile(info().add(k))
}

/// The table of `core/dynfx`, or null (also when it is for another device or has another row size than the runtime knows).
pub(crate) unsafe fn finfo() -> *const u32 {
    #[cfg(test)] { return crate::cleanup_tests::fx_info(); }
    #[cfg(not(test))] {
        let a = fn_addr(dynfx_info);
        if a == 0 || info().is_null() {
            return core::ptr::null();
        }
        let p = a as *const u32;
        if read_volatile(p.add(F_MAGIC)) != FX_INFO_MAGIC
            || read_volatile(p.add(F_DEVICE)) != iw(I_DEVICE)
            || read_volatile(p.add(F_WORDS)) != FX_ROW_WORDS
        {
            return core::ptr::null();
        }
        p
    }
}

pub(crate) unsafe fn have_link() -> bool {
    fn_addr(dl_send) != 0 && fn_addr(dl_send_win) != 0 && fn_addr(dl_poll) != 0 && !info().is_null()
}

pub(crate) unsafe fn call_send(dst: u32, buf: *const u8, len: u32) -> i32 {
    if retained() { return -2; }
    #[cfg(test)] { return crate::cleanup_tests::send_image(dst, buf, len); }
    #[cfg(not(test))] {
        let f: unsafe extern "C" fn(u32, *const u8, u32) -> i32 = core::mem::transmute(fn_addr(dl_send));
        f(dst, buf, len)
    }
}

pub(crate) unsafe fn call_send_win(win: u32, dst: u32, buf: *const u32, len: u32) -> i32 {
    #[cfg(test)] { return crate::cleanup_tests::send_word(win, dst, buf, len); }
    #[cfg(not(test))] {
        let f: unsafe extern "C" fn(u32, u32, *const u32, u32) -> i32 =
            core::mem::transmute(fn_addr(dl_send_win));
        f(win, dst, buf, len)
    }
}

pub(crate) unsafe fn call_poll(s: *mut DlStatus) -> i32 {
    #[cfg(test)] {
        let (state, err) = crate::cleanup_tests::poll_state();
        (*s).state = state;
        (*s).err = err;
        return 0;
    }
    #[cfg(not(test))] {
        let f: unsafe extern "C" fn(*mut DlStatus) -> i32 = core::mem::transmute(fn_addr(dl_poll));
        f(s)
    }
}

pub unsafe fn init_once() {
    let d = ds();
    if d.magic == STATE_MAGIC {
        return;
    }
    let mut i = 0;
    while i < MAX_ROWS {
        d.row_type[i] = FREE_TYPE;
        i += 1;
    }
    d.magic = STATE_MAGIC;
}

// ---- the area ----------------------------------------------------------------------------------------------------------

/// Bytes of the area (0: the image has no DSP loader).
pub unsafe fn area_bytes() -> u32 {
    if info().is_null() {
        0
    } else {
        iw(I_AREA_BYTES)
    }
}

/// Free bytes in the area and the largest free range, for `dkrt_status`.
pub unsafe fn area_stats() -> (u32, u32) {
    let total = area_bytes();
    if total == 0 {
        return (0, 0);
    }
    let d = ds();
    let mut used = 0;
    let mut i = 0;
    while i < crate::MAX_MODULES {
        used += d.mod_len[i];
        i += 1;
    }
    let mut largest = 0;
    let mut cursor = 0u32;
    loop {
        let mut next = total;
        i = 0;
        while i < crate::MAX_MODULES {
            if d.mod_len[i] != 0 && d.mod_off[i] >= cursor && d.mod_off[i] < next {
                next = d.mod_off[i];
            }
            i += 1;
        }
        if next - cursor > largest {
            largest = next - cursor;
        }
        if next >= total {
            break;
        }
        let mut len = 0;
        i = 0;
        while i < crate::MAX_MODULES {
            if d.mod_len[i] != 0 && d.mod_off[i] == next {
                len = d.mod_len[i];
            }
            i += 1;
        }
        cursor = next + len;
    }
    (total - used, largest)
}

/// First fit: the lowest offset whose byte address is `align`-aligned and whose `len` bytes touch no other range.
pub(crate) unsafe fn alloc_range(len: u32, align: u32) -> Option<u32> {
    let d = ds();
    capacity::first_fit(iw(I_AREA), iw(I_AREA_BYTES), &d.mod_off, &d.mod_len, len, align)
}

// ---- checking a section --------------------------------------------------------------------------------------------------

/// Bytes of the range a module needs, before the granule: image, bss, then (version 3) the state blocks of its voices, every block
/// of `voices * state_bytes` rounded to 8 bytes, then (version 4) the state of each effect rounded to 8. The caller has checked the counts, so nothing overflows.
pub(crate) unsafe fn extent(image_size: u32, bss: u32, voices: *const u8, nvoice: u32, vstride: u32, fxs: *const u8, nfx: u32) -> u32 {
    let mut total = (image_size + bss + 7) & !7;
    if vstride == VOICE_V3 {
        let mut i = 0;
        while i < nvoice {
            let e = voices.add((vstride * i) as usize);
            total += rd32(e.add(52)) * ((rd32(e.add(12)) + 7) & !7);
            i += 1;
        }
    }
    // then the state of each effect (version 4)
    let mut i = 0;
    while i < nfx {
        total += (rd32(fxs.add((FX_REC * i) as usize + 8)) + 7) & !7;
        i += 1;
    }
    total
}

/// An exported DSP symbol of `dyndsp_info`; 0 when there is none of that name.
pub(crate) unsafe fn dsp_symbol(name: *const u8) -> u32 {
    let n = iw(I_NIMPORT);
    let mut i = 0;
    while i < n {
        let e = (info().add(I_IMPORTS) as *const u8).add(32 * i as usize);
        if cstr_eq(e, name) {
            return read_volatile(e.add(28) as *const u32);
        }
        i += 1;
    }
    0
}

/// Validate the DSP section `[off, off + size)` of the staged file `f` for this runtime and image. Writes only `job`.
pub unsafe fn check(f: *const u8, off: u32, size: u32, file_size: u32, job: &mut Job) -> i32 {
    init_once();
    if !in_sec(off, size, file_size) || size < 0x40 {
        return E_DSP_FORMAT;
    }
    let s = f.add(off as usize);
    let version = rd32(s.add(4));
    if rd32(s) != STATE_MAGIC || (version != 1 && version != 2 && version != 3 && version != 4 && version != 5) {
        return E_DSP_FORMAT;
    }
    if !have_link() {
        return E_NO_DSP;
    }
    if rd32(s.add(8)) != iw(I_DEVICE) {
        return E_DSP_DEVICE;
    }
    let image_size = rd32(s.add(0x0c));
    let bss = rd32(s.add(0x10));
    let align = rd32(s.add(0x14));
    let (image_off, reloc_off, nreloc) = (rd32(s.add(0x18)), rd32(s.add(0x1c)), rd32(s.add(0x20)));
    let (import_off, nimport) = (rd32(s.add(0x24)), rd32(s.add(0x28)));
    let (voice_off, nvoice) = (rd32(s.add(0x2c)), rd32(s.add(0x30)));
    // version 4: the effect records (0x34 offset, 0x38 count)
    let (fx_off, nfx) = if version >= 4 { (rd32(s.add(0x34)), rd32(s.add(0x38))) } else { (0, 0) };
    let vstride = match version {
        1 => VOICE_V1,
        2 => VOICE_V2,
        _ => VOICE_V3,
    };
    if image_size > iw(I_AREA_BYTES)
        || bss > iw(I_AREA_BYTES)
        || image_size == 0
        || image_size & 3 != 0
        || bss & 3 != 0
        || !(8..=GRANULE).contains(&align)
        || align & (align - 1) != 0
        || nreloc > MAX_RELOCS
        || nimport > MAX_IMPORTS
        || nvoice as usize > MAX_VOICES
        || nfx as usize > MAX_FX
        || !in_sec(fx_off, nfx * FX_REC, size)
        || !in_sec(image_off, image_size, size)
        || !in_sec(reloc_off, nreloc * 8, size)
        || !in_sec(import_off, nimport * 32, size)
        || !in_sec(voice_off, nvoice * vstride, size)
    {
        return E_DSP_FORMAT;
    }
    let bank_off = if version == 5 { rd32(s.add(0x3c)) } else { 0 };
    let sec = DspSec {
        s,
        version,
        size,
        image_size,
        image_off,
        reloc_off,
        nreloc,
        import_off,
        nimport,
        voice_off,
        nvoice,
        fx_off,
        nfx,
        vstride,
        bank_off,
    };
    // the entries of the tables the services own: track table, voices, effects
    let r = Services::dsp_check_sections(&sec);
    if r != OK {
        return r;
    }
    // the range must fit the area at all (a later load may still find it taken: reserve())
    let need = extent(image_size, bss, s.add(voice_off as usize), nvoice, vstride, s.add(fx_off as usize), nfx);
    if need > iw(I_AREA_BYTES) || (need + GRANULE - 1) & !(GRANULE - 1) > iw(I_AREA_BYTES) {
        return E_DSP_NOSPACE;
    }
    let mut i = 0;
    while i < nimport {
        let p = s.add(import_off as usize + 32 * i as usize);
        if cstr_len(p, 32) == 32 {
            return E_DSP_FORMAT;
        }
        let v = dsp_symbol(p);
        if v == 0 {
            return E_DSP_IMPORT;
        }
        job.import_vals[i as usize] = v;
        i += 1;
    }
    i = 0;
    while i < nreloc {
        let e = s.add(reloc_off as usize + 8 * i as usize);
        let o = rd32(e);
        let w = rd32(e.add(4));
        let (kind, class, imp) = (w >> 28, (w >> 24) & 15, w & 0xffff);
        if o > image_size - 6 || kind > 1 || class > 2 || (class == 2 && imp >= nimport) || (kind == 0 && class == 1) {
            return E_DSP_FORMAT;
        }
        i += 1;
    }
    // voice and effect rows, then the voices' types
    let r = Services::dsp_check_rows(&sec);
    if r != OK {
        return r;
    }
    let r = Services::dsp_check_types(&sec);
    if r != OK {
        return r;
    }
    job.image = s.add(image_off as usize);
    job.image_size = image_size;
    job.bss = bss;
    job.align = align;
    job.relocs = s.add(reloc_off as usize);
    job.nreloc = nreloc;
    job.imports = s.add(import_off as usize);
    job.nimport = nimport;
    job.voices = s.add(voice_off as usize);
    job.nvoice = nvoice;
    job.vstride = vstride;
    job.fxs = s.add(fx_off as usize);
    job.nfx = nfx;
    job.bank = if version == 5 { s.add(bank_off as usize) } else { core::ptr::null() };
    OK
}

// ---- reserving: range, relocated image, voice rows ------------------------------------------------------------------------

pub(crate) unsafe fn le16(p: *const u8) -> u32 {
    (*p as u32) | ((*p.add(1) as u32) << 8)
}

pub(crate) unsafe fn put_le16(p: *mut u8, v: u32) {
    *p = v as u8;
    *p.add(1) = (v >> 8) as u8;
}

/// Reserve everything a module's DSP part needs: a range in the area, an arena block with the relocated image, the voice rows.
/// Nothing is sent yet. On an error nothing is left reserved.
pub unsafe fn reserve(rec: usize, job: &Job) -> i32 {
    if retained() { return E_BUSY; }
    // a service may need a slot of its own: refuse before anything is reserved
    let r = Services::dsp_admit(job);
    if r != OK {
        return r;
    }
    let d = ds();
    let len = (extent(job.image_size, job.bss, job.voices, job.nvoice, job.vstride, job.fxs, job.nfx) + GRANULE - 1) & !(GRANULE - 1);
    let off = match alloc_range(len, job.align) {
        Some(o) => o,
        None => return E_DSP_NOSPACE,
    };
    let buf = arena::alloc(job.image_size);
    if buf.is_null() {
        return E_NOMEM;
    }
    let base = iw(I_AREA) + off;
    // copy, then relocate: abs24 / abs32 fields of 48-bit instructions, shifted by where the object sits in L2
    let mut i = 0;
    while i < job.image_size as usize {
        *buf.add(i) = *job.image.add(i);
        i += 1;
    }
    let pc_shift = (base - L2_BYTE_BASE) / 2;
    let byte_shift = base - L2_BYTE_BASE;
    i = 0;
    while i < job.nreloc as usize {
        let e = job.relocs.add(8 * i);
        let o = rd32(e) as usize;
        let w = rd32(e.add(4));
        let (kind, class, imp) = (w >> 28, (w >> 24) & 15, (w & 0xffff) as usize);
        let add = match class {
            0 => pc_shift,
            1 => byte_shift,
            _ => job.import_vals[imp],
        };
        let p = buf.add(o);
        if kind == 0 {
            let v = (((*p.add(2)) as u32) << 16 | le16(p.add(4))).wrapping_add(add);
            if v >= 1 << 24 {
                arena::free(buf);
                return E_DSP_FORMAT;
            }
            *p.add(2) = (v >> 16) as u8;
            put_le16(p.add(4), v & 0xffff);
        } else {
            let v = (le16(p.add(2)) << 16 | le16(p.add(4))).wrapping_add(add);
            put_le16(p.add(2), v >> 16);
            put_le16(p.add(4), v & 0xffff);
        }
        i += 1;
    }
    // the services claim their rows: the voice states follow image and bss (8-aligned), then the effect states, in record order
    let mut claim = Claim { rec, job, base, byte_shift, state_at: (job.image_size + job.bss + 7) & !7 };
    Services::dsp_claim(&mut claim);
    d.mod_off[rec] = off;
    d.mod_len[rec] = len;
    d.send_base = base;
    d.send_len = job.image_size;
    d.send_buf = buf as u32;
    OK
}

/// Give back what `reserve` took for module record `rec` (the rows must have been cleared first).
pub(crate) unsafe fn unreserve(rec: usize) {
    Services::dsp_release(rec);
    let d = ds();
    d.mod_off[rec] = 0;
    d.mod_len[rec] = 0;
}

/// Undo a `reserve` that was never started (the CF side failed after it).
pub unsafe fn cancel(rec: usize) {
    if retained() { return; }
    let d = ds();
    if d.send_buf != 0 {
        arena::free(d.send_buf as *mut u8);
        d.send_buf = 0;
    }
    unreserve(rec);
}

// ---- one-word writes into the voices registry ---------------------------------------------------------------------------------

/// Start writing `value` into word `word` of window `win` (1 the voice rows, 2 the effect rows); false when the link refused (busy: try again).
pub(crate) unsafe fn start_word(win: u32, word: u32, value: u32) -> bool {
    if retained() { return false; }
    let d = ds();
    // the link sends the bytes of the buffer as little-endian DSP words; this is a big-endian ColdFire long
    d.word = (value << 24) | ((value & 0xff00) << 8) | ((value >> 8) & 0xff00) | (value >> 24); // byte swap by hand: no libgcc here
    call_send_win(win, word, &d.word as *const u32, 4) == 0
}

/// `Some(true)` done, `Some(false)` failed, `None` still running.
pub(crate) unsafe fn poll_done() -> Option<bool> {
    let mut s = DlStatus {
        state: 0,
        err: 0,
        nchunks: 0,
        acked: 0,
        sent: 0,
        resent: 0,
        nerr: 0,
    };
    call_poll(&mut s);
    match s.state {
        DL_DONE => Some(true),
        DL_ERROR => {
            ds().last_dl_err = s.err;
            Some(false)
        }
        _ => None,
    }
}

/// Write one word and wait for it (the UI task spins; a few audio blocks). Used on the error paths.
pub(crate) unsafe fn write_word_blocking(win: u32, word: u32, value: u32) -> bool {
    let mut n = 0;
    while !start_word(win, word, value) {
        n += 1;
        if n > SPIN {
            return false;
        }
        core::hint::black_box(n);
    }
    ds().row_writes += 1;
    n = 0;
    loop {
        match poll_done() {
            Some(ok) => return ok,
            None => {
                n += 1;
                if n > SPIN {
                    return false;
                }
                core::hint::black_box(n);
            }
        }
    }
}

/// Invalidate published DSP rows; does not release code/state or transfer sources.
pub unsafe fn quiesce(rec: usize) -> bool {
    if retained() { return false; }
    if ds().mod_len[rec] == 0 { return true; }
    Services::dsp_clear(rec)
}

/// Release only after the caller has confirmed invalidation and CF admission/drain.
pub unsafe fn release_range(rec: usize) {
    if !retained() { unreserve(rec); }
}

pub unsafe fn release(rec: usize) -> bool {
    if !quiesce(rec) { ds().phase = PH_RETAINED; return false; }
    release_range(rec);
    true
}

// ---- the state machine --------------------------------------------------------------------------------------------------------

/// Hand a load over to the state machine: the CF side is complete and described by `p`; `reserve` has run for `p.rec`.
pub unsafe fn launch(mut p: Pending) {
    // A prepared reservation, if any, must remain owned after global retention.
    if retained() { return; }
    let d = ds();
    p.send_base = d.send_base;
    p.send_len = d.send_len;
    p.send_buf = d.send_buf;
    d.send_buf = 0;
    if d.phase == PH_IDLE && d.nwait == 0 {
        start(p);
    } else {
        d.waiting[d.nwait as usize] = p;
        d.nwait += 1;
    }
}

pub(crate) unsafe fn start(p: Pending) {
    let d = ds();
    d.pending = p;
    d.phase = PH_SEND;
    d.ticks = 0;
    d.step = 0;
    d.started = 0;
    d.transfers += 1;
}

/// The load in progress is over: start the next waiting one, or go idle.
pub(crate) unsafe fn next_job() {
    if retained() { return; }
    let d = ds();
    if d.nwait > 0 {
        let p = d.waiting[0];
        let mut k = 1;
        while k < d.nwait as usize {
            d.waiting[k - 1] = d.waiting[k];
            k += 1;
        }
        d.nwait -= 1;
        start(p);
    } else {
        d.phase = PH_IDLE;
    }
}

/// What one pass of `step` found: the loader (`loader::dsp_step`) acts on a result, then calls back `fail_done` or `loaded_done`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Advance {
    /// Nothing to settle: idle, waiting for the DSP, or a step done.
    Pending,
    /// The load in progress failed with this code. The loader takes the module down, then calls `fail_done`.
    Failed(i32),
    /// Every row of the load in progress is in. The loader lists the module, then calls `loaded_done`.
    Loaded,
}

/// The loader took the failed load's module down (`reclaimed`: it did, else the DSP state stays retained) and told the boot: finish the failure.
pub(crate) unsafe fn fail_done(reclaimed: bool, code: i32) {
    let d = ds();
    d.failures += 1;
    crate::st().last_result = code;
    if !reclaimed {
        d.phase = PH_RETAINED;
        return;
    }
    if d.pending.send_buf != 0 {
        arena::free(d.pending.send_buf as *mut u8);
        d.pending.send_buf = 0;
    }
    next_job();
}

/// The loader listed the loaded module and told the boot: start the next waiting load or go idle.
pub(crate) unsafe fn loaded_done() {
    crate::st().last_result = OK;
    next_job();
}

/// Advance the load in progress by what is ready now. Called once per UI-loop pass.
pub unsafe fn step() -> Advance {
    step_with::<Services>()
}

/// `step` over a given list of services.
pub(crate) unsafe fn step_with<L: ServiceList>() -> Advance {
    let d = ds();
    if d.magic != STATE_MAGIC {
        return Advance::Pending;
    }
    if d.phase == PH_IDLE || d.phase == PH_RETAINED {
        return Advance::Pending;
    }
    d.ticks = d.ticks.saturating_add(1);
    if d.phase == PH_QUARANTINE {
        if poll_done().is_some() {
            d.started = 0;
            return Advance::Failed(E_DSP_XFER);
        }
        return Advance::Pending;
    }
    if d.ticks > GUARD_TICKS {
        if d.started != 0 && poll_done().is_none() {
            d.phase = PH_QUARANTINE;
            return Advance::Pending;
        }
        return Advance::Failed(E_DSP_XFER);
    }
    loop {
        match d.phase {
            PH_SEND => {
                if d.started == 0 {
                    let r = call_send(d.pending.send_base, d.pending.send_buf as *const u8, d.pending.send_len);
                    if r == -2 {
                        return Advance::Failed(E_DSP_XFER);
                    }
                    if r == 0 {
                        d.started = 1;
                    }
                    return Advance::Pending;
                }
                match poll_done() {
                    None => return Advance::Pending,
                    Some(false) => {
                        return Advance::Failed(E_DSP_XFER);
                    }
                    Some(true) => {
                        arena::free(d.pending.send_buf as *mut u8);
                        d.pending.send_buf = 0;
                        d.phase = PH_ROWS;
                        d.step = 0;
                        d.started = 0;
                        d.ticks = 0;
                    }
                }
            }
            PH_ROWS => {
                // the rows of the module in claim order, each a run of steps that the services declare (`Service::dsp_row_step`)
                let mut want = d.step;
                let write = match L::dsp_row_step(d.pending.rec as usize, &mut want, d.started == 0) {
                    Some(w) => w,
                    None => {
                        // every row is in: list the machine
                        return Advance::Loaded;
                    }
                };
                if d.started == 0 {
                    if start_word(write.win, write.word, write.value) {
                        d.started = 1;
                        d.row_writes += 1;
                        if let Some(flag) = write.published {
                            *flag = 1;
                        }
                    }
                    return Advance::Pending;
                }
                match poll_done() {
                    None => return Advance::Pending,
                    Some(false) => {
                        return Advance::Failed(E_DSP_XFER);
                    }
                    Some(true) => {
                        d.step += 1;
                        d.started = 0;
                        d.ticks = 0;
                    }
                }
            }
            _ => return Advance::Pending,
        }
    }
}

pub unsafe fn state() -> u32 {
    if ds().phase != PH_IDLE {
        STATE_LOADING
    } else {
        STATE_IDLE
    }
}

#[cfg(test)]
pub(super) unsafe fn cleanup_test_first_fit(len: u32, align: u32) -> Option<u32> {
    alloc_range(len, align)
}
