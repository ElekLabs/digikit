//! Effect-row part of the DSP loader: the chain rows, their owners, one-word writes and the clearing of a module's effect rows.
use crate::rows::frows;
use core::ptr::addr_of_mut;
use dkneeds::fx;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::Job;
use dkrt_abi::dsp::*;
use dkrt_abi::link::{Owner, Sequence, Slot};
use dkrt_abi::mem::{iw, rd32};
use dkrt_abi::service::{Claim, RowWrite, TableFault};
use dkrt_sdk::fault::{DIAG_WORDS, SITE_VOICE};
use dkrt_sdk::*;

/// `DFXI`, the first word of `dynfx_info`.
const DYNFX_NAME: &str = "dynfx_info";
const DYNDSP_NAME: &str = "dyndsp_info";

/// The words of `dynfx_info` (the effect chain's table, from the image's info table); null without it, and when it is for another device or has another row size than
/// this service knows.
pub unsafe fn dynfx<C: CoreApi>(core: &C) -> *const u32 {
    let t = core.info(dkneeds::keys::name_hash(DYNFX_NAME));
    if t.is_null()
        || iw(t, F_MAGIC) != FX_INFO_MAGIC
        || iw(t, F_DEVICE) != core.device_id()
        || iw(t, F_WORDS) != FX_ROW_WORDS
    {
        return core::ptr::null();
    }
    t
}

/// The words of `dyndsp_info` (the DSP area), for the bounds of an effect's state; null without it.
unsafe fn dyndsp<C: CoreApi>(core: &C) -> *const u32 {
    let t = core.info(dkneeds::keys::name_hash(DYNDSP_NAME));
    if t.is_null() || iw(t, I_MAGIC) != INFO_MAGIC {
        return core::ptr::null();
    }
    t
}

/// Word `k` of `dynfx_info`.
unsafe fn fxiw<C: CoreApi>(core: &C, k: usize) -> u32 {
    iw(dynfx(core), k)
}

/// Effect rows the image reserves for run-time effects (0 without `core/dynfx`).
pub unsafe fn fx_free_rows<C: CoreApi>(core: &C) -> u32 {
    if dynfx(core).is_null() {
        0
    } else {
        fxiw(core, F_FREE).min(MAX_FX as u32)
    }
}

/// Effect rows in use (claimed by a module that is loaded or loading), for CAPACITY.
pub unsafe fn fx_used<C: CoreApi>(_core: &C) -> u32 {
    let d = frows();
    let mut used = 0;
    let mut k = 0;
    while k < MAX_FX {
        if d.fx_owner[k] != 0 {
            used += 1;
        }
        k += 1;
    }
    used
}

/// The module record + 1 that owns effect row `k` (0: free) and the row's index in the chain's table (the registry `fx`), for the effect pages.
pub unsafe fn fx_owner_of<C: CoreApi>(_core: &C, k: usize) -> u8 {
    if k < MAX_FX {
        frows().fx_owner[k]
    } else {
        0
    }
}

/// The effect row (0 ..) that module record `owner - 1` claimed for its `ordinal`-th effect record (rows are claimed in record order, lowest first).
pub unsafe fn fx_row_of<C: CoreApi>(_core: &C, owner: u8, ordinal: u32) -> Option<usize> {
    let d = frows();
    let mut seen = 0;
    let mut k = 0;
    while k < MAX_FX {
        if d.fx_owner[k] == owner {
            if seen == ordinal {
                return Some(k);
            }
            seen += 1;
        }
        k += 1;
    }
    None
}

/// What `init` declared for effect record `ordinal` of module `rec`: its route flags (`Some`) and the defaults of the words p0 to p7 (`Some`).
pub unsafe fn fx_set_declared<C: CoreApi>(
    core: &C,
    rec: usize,
    ordinal: u32,
    flags: Option<u32>,
    par: Option<[u32; 8]>,
) -> bool {
    match fx_row_of(core, rec as u8 + 1, ordinal) {
        Some(k) => {
            let d = frows();
            if let Some(f) = flags {
                d.fx_flags[k] = f;
            }
            if let Some(p) = par {
                d.fx_par[k] = p;
            }
            true
        }
        None => false,
    }
}

/// The flags word of the record that effect row `k` was claimed for (`FXF_SEND`, `FXF_NOMASTER`, `FXF_DEFAULT_SEND`; 0 for a free row).
pub unsafe fn fx_flags_of<C: CoreApi>(_core: &C, k: usize) -> u32 {
    if k < MAX_FX {
        frows().fx_flags[k]
    } else {
        0
    }
}

/// Set the MIX word an effect row publishes (before its rows go out) and the defaults of its parameter words are the record's.
pub unsafe fn fx_set_mix<C: CoreApi>(_core: &C, k: usize, bits: u32) {
    if k < MAX_FX {
        frows().fx_mix[k] = bits;
    }
}

/// Set the route and the four amount words an effect row publishes (before its rows go out; `fxsend::apply` does it from the settings).
pub unsafe fn fx_set_route<C: CoreApi>(_core: &C, k: usize, route: u32, amt: [u32; 4]) {
    if k < MAX_FX {
        let d = frows();
        d.fx_route[k] = route;
        d.fx_amt[k] = amt;
    }
}

/// An effect row whose words are in the DSP: claimed, published and no load in progress, so a live value can be set.
pub unsafe fn fx_live<C: CoreApi>(core: &C, k: usize) -> bool {
    let (d, r) = (core.dsp(), frows());
    k < MAX_FX && r.fx_owner[k] != 0 && r.fx_pub[k] != 0 && d.phase == PH_IDLE && d.nwait == 0
}

/// The link slot of word `w` of effect row `k` (window 2); `None` without the effect table.
pub unsafe fn fx_slot<C: CoreApi>(c: &C, k: usize, w: u32) -> Option<Slot> {
    let inf = dynfx(c);
    if inf.is_null() {
        return None;
    }
    Some(Slot::new(
        WIN_FX,
        FX_ROW_WORDS * (iw(inf, F_FIRST) + k as u32) + w,
    ))
}

/// Drop the live values of effect row `k` (its module is gone): the link sends nothing more to the row.
pub unsafe fn fx_forget<C: CoreApi>(c: &C, k: usize) {
    if let Some(s) = fx_slot(c, k, 0) {
        c.link_forget(WIN_FX, s.word, FX_ROW_WORDS);
    }
}

/// The module record (index) that owns row `r` of the effect table (the registry `fx`), for an effect fault.
pub unsafe fn module_of_fx_table_row<C: CoreApi>(core: &C, r: u32) -> Option<usize> {
    if dynfx(core).is_null() {
        return None;
    }
    let first = fxiw(core, F_FIRST);
    if r < first || ((r - first) as usize) >= MAX_FX {
        return None;
    }
    let owner = frows().fx_owner[(r - first) as usize];
    if owner == 0 {
        None
    } else {
        Some(owner as usize - 1)
    }
}

/// Free the effect rows module record `rec` published: the pc first, so the chain skips the row at once; then the adapter's words (`was`, `dis`, the fault
/// window and its count) back to 0, so the next effect on the row starts clean.
pub(crate) unsafe fn clear_fx_rows<C: CoreApi>(core: &C, rec: usize) -> bool {
    let d = frows();
    if dynfx(core).is_null() {
        // Missing info is harmless only when no owned row was published.
        let mut k = 0;
        while k < MAX_FX {
            if d.fx_owner[k] == rec as u8 + 1 && d.fx_pub[k] != 0 {
                return false;
            }
            k += 1;
        }
        return true;
    }
    let first = fxiw(core, F_FIRST);
    let mut k = 0;
    while k < MAX_FX {
        if d.fx_owner[k] == rec as u8 + 1 && d.fx_pub[k] != 0 {
            let at = FX_ROW_WORDS * (first + k as u32);
            // one sequence per row: the pc first, so the chain skips the row at once, then the rest; the next effect on the row starts on the master route with
            // no amounts (finding 62)
            let mut seq = Sequence::new();
            seq.push(WIN_FX, at + FXW_PC, FX_FREE_PC);
            seq.push(WIN_FX, at + FXW_ROUTE, 0);
            let mut w = 0;
            while w < 4 {
                seq.push(WIN_FX, at + FXW_AMT0 + w, 0);
                w += 1;
            }
            seq.push(WIN_FX, at + FXW_WAS, 0);
            seq.push(WIN_FX, at + FXW_MIX, MIX_ONE);
            seq.push(WIN_FX, at + FXW_DIS, 0);
            seq.push(WIN_FX, at + FXW_WSTART, 0);
            seq.push(WIN_FX, at + FXW_WCOUNT, 0);
            fx_forget(core, k); // no live value may reach the row after its clear
            if !core.link_run(Owner::Core, &seq) {
                return false;
            }
            d.fx_pub[k] = 0;
        }
        k += 1;
    }
    true
}

/// Bytes of the DSP area (0 without `core/dyndsp`).
unsafe fn area_bytes<C: CoreApi>(core: &C) -> u32 {
    let t = dyndsp(core);
    if t.is_null() {
        0
    } else {
        iw(t, I_AREA_BYTES)
    }
}

/// DSP section, effect records: the count and the table inside the section, entries and descriptor inside the image, the descriptor says what the record says.
/// The state of each effect (rounded to 8) is added to `job.state_bytes`.
pub(crate) unsafe fn check_sections<C: CoreApi>(core: &C, sec: &DspSec, job: &mut Job) -> i32 {
    let (s, fx_off, nfx, image_size, image_off) =
        (sec.s, sec.fx_off, sec.nfx, sec.image_size, sec.image_off);
    if nfx as usize > MAX_FX || !in_sec(fx_off, nfx * FX_REC, sec.size) {
        return E_DSP_FORMAT;
    }
    let mut i = 0;
    while i < nfx {
        let e = s.add((fx_off + FX_REC * i) as usize);
        let (process, desc, state, np) = (
            rd32(e.add(FX_PROCESS as usize)),
            rd32(e.add(FX_DESC as usize)),
            rd32(e.add(FX_STATE as usize)),
            rd32(e.add(FX_NPARAMS as usize)),
        );
        if process >= image_size
            || process & 1 != 0
            || desc & 3 != 0
            || !in_sec(desc, FX_DESC_BYTES, image_size)
            || state & 3 != 0
            || state > area_bytes(core)
            || np > FX_MAX_PARAMS
        {
            return E_DSP_FORMAT;
        }
        // the routes: send allowed (bit 0), master not allowed (bit 1), default send (bit 2); the default must be an allowed route (0 is a record before the routes)
        let fl = rd32(e.add(FX_FLAGS as usize));
        if fl & !7 != 0 || (fl & 6 != 0 && fl & 1 == 0) || (fl & 2 != 0 && fl & 4 == 0) {
            return E_DSP_FORMAT;
        }
        let dd = s.add((image_off + desc) as usize);
        if rd32le(dd.add(fx::DK_FX_DESC_MAGIC as usize)) != FX_DESC_MAGIC
            || rd32le(dd.add(fx::DK_FX_DESC_CONTRACT as usize)) != fx::DK_FX_V1
            || rd32le(dd.add(fx::DK_FX_DESC_N_PARAMS as usize)) != np
            || rd32le(dd.add(fx::DK_FX_DESC_STATE_BYTES as usize)) != state
        {
            return E_DSP_FORMAT;
        }
        job.state_bytes += (state + 7) & !7;
        i += 1;
    }
    job.accepted += nfx;
    OK
}

/// The effect rows (below the image's limit) that the module being reloaded owns (`CoreApi::live_op`: its record + 1; 0 when no live job runs).
unsafe fn own_rows<C: CoreApi>(core: &C) -> u32 {
    let (d, op, limit) = (frows(), core.live_op() as u8, fx_free_rows(core));
    let mut n = 0;
    let mut k = 0;
    while op != 0 && k < MAX_FX {
        if d.fx_owner[k] == op && (k as u32) < limit {
            n += 1;
        }
        k += 1;
    }
    n
}

/// DSP section: the image must have the chain's table, and a free row for each record.
pub(crate) unsafe fn check_rows<C: CoreApi>(core: &C, sec: &DspSec) -> i32 {
    let nfx = sec.nfx;
    let d = frows();
    // effect rows: the image must have the chain's table, and a free row for each record
    if nfx > 0 {
        if dynfx(core).is_null() {
            return E_NO_DSP;
        }
        // the module being reloaded (`CoreApi::live_op`) still owns its rows at the check: the teardown that comes before the load frees them, so they count as free
        let free = capacity::free_rows(&d.fx_owner, fx_free_rows(core)) + own_rows(core);
        let r = capacity::dsp_rows(nfx, free);
        if r != OK {
            return r;
        }
    }
    OK
}

/// Claim the effect rows (lowest free ones first); the states follow the voice states, in record order.
pub(crate) unsafe fn claim<C: CoreApi>(core: &C, c: &mut Claim) {
    let (rec, job, base) = (c.rec, c.job, c.base);
    let d = frows();
    let mut state_at = c.state_at;
    let mut fclaimed = 0u32;
    let mut f = 0;
    let mut frow = 0;
    while f < job.nfx as usize {
        while frow < MAX_FX && (d.fx_owner[frow] != 0 || (frow as u32) >= fx_free_rows(core)) {
            frow += 1;
        }
        let e = job.fxs.add(FX_REC as usize * f);
        d.fx_pc[frow] = L2_SW_BASE + (base + rd32(e.add(FX_PROCESS as usize)) - L2_BYTE_BASE) / 2;
        d.fx_desc[frow] = base + rd32(e.add(FX_DESC as usize));
        d.fx_state[frow] = base + state_at;
        state_at += (rd32(e.add(FX_STATE as usize)) + 7) & !7;
        let mut k = 0;
        while k < 8 {
            d.fx_par[frow][k] = rd32(e.add(FX_PARAMS as usize + 4 * k));
            k += 1;
        }
        d.fx_mix[frow] = MIX_ONE;
        d.fx_route[frow] = 0;
        d.fx_amt[frow] = [0; 4];
        d.fx_flags[frow] = rd32(e.add(FX_FLAGS as usize));
        d.fx_owner[frow] = rec as u8 + 1;
        d.fx_pub[frow] = 0;
        fclaimed |= 1 << frow;
        frow += 1;
        f += 1;
    }
    c.state_at = state_at;
    d.mod_fx[rec] = fclaimed;
}

/// Give back the effect rows module record `rec` claimed.
pub(crate) unsafe fn release<C: CoreApi>(_core: &C, rec: usize) {
    let d = frows();
    let mut k = 0;
    while k < MAX_FX {
        if d.fx_owner[k] == rec as u8 + 1 {
            d.fx_owner[k] = 0;
            d.fx_pub[k] = 0;
            d.fx_pc[k] = 0;
            d.fx_desc[k] = 0;
            d.fx_state[k] = 0;
            d.fx_par[k] = [0; 8];
            d.fx_mix[k] = MIX_ONE;
            d.fx_route[k] = 0;
            d.fx_amt[k] = [0; 4];
            d.fx_flags[k] = 0;
        }
        k += 1;
    }
    d.mod_fx[rec] = 0;
}

/// The effect-row word to publish at step `*want` of module `rec`: a row takes `FX_STEPS` steps (the 8 parameter words, desc, state,
/// mix, enabled, the 4 amount words, the route, the pc).
pub(crate) unsafe fn row_step<C: CoreApi>(
    core: &C,
    rec: usize,
    want: &mut u32,
    write: bool,
) -> Option<RowWrite> {
    let d = frows();
    let mut frow = MAX_FX;
    let mut k = 0;
    while k < MAX_FX {
        if d.fx_owner[k] == rec as u8 + 1 {
            if *want < FX_STEPS {
                frow = k;
                break;
            }
            *want -= FX_STEPS;
        }
        k += 1;
    }
    if frow == MAX_FX {
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
    let want = *want;
    let at = FX_ROW_WORDS * (fxiw(core, F_FIRST) + frow as u32);
    let (word, value) = if want < 8 {
        (at + FXW_P0 + want, d.fx_par[frow][want as usize])
    } else if want == 8 {
        (at + FXW_DESC, d.fx_desc[frow])
    } else if want == 9 {
        (at + FXW_STATE, d.fx_state[frow])
    } else if want == 10 {
        (at + FXW_MIX, d.fx_mix[frow])
    } else if want == 11 {
        (at + FXW_ENABLED, 1)
    } else if want < FX_STEPS - 2 {
        (
            at + FXW_AMT0 + (want - FX_STEP_ROUTE),
            d.fx_amt[frow][(want - FX_STEP_ROUTE) as usize],
        )
    } else if want == FX_STEPS - 2 {
        (at + FXW_ROUTE, d.fx_route[frow])
    } else {
        (at + FXW_PC, d.fx_pc[frow])
    };
    // from the amounts on, the amounts, the route or the pc word may be in the DSP: clear them on failure
    let published = if want >= FX_STEP_ROUTE {
        Some(&mut d.fx_pub[frow])
    } else {
        None
    };
    // the pc word makes the row visible: it ends the row, and a failed sequence frees it again
    let last = want == FX_STEPS - 1;
    Some(RowWrite {
        win: WIN_FX,
        word,
        value,
        published,
        last,
        has_undo: last,
        undo: if last { FX_FREE_PC } else { 0 },
    })
}

// ---- the checks of the table, the diagnostics words and the fault sites -----------------------------------------------------

/// Bytes of a row of the effect table, as the DSP reports a row (its byte offset in the table).
const ROW_BYTES: u32 = FX_ROW_WORDS * 4;

/// The effect table against the module records (`Service::table_check`): an owned row belongs to a used record whose `mod_fx` has its bit, and every bit of `mod_fx` is
/// a row the record owns and the record is used. The mismatch is in `out` as (record, `0x400 | row`, the row's owner).
pub(crate) unsafe fn table_check(used: u32, out: &mut TableFault) -> u32 {
    let r = frows();
    let mut k = 0;
    while k < MAX_FX {
        let o = r.fx_owner[k];
        if o != 0
            && (o as usize > dkrt_abi::MAX_MODULES
                || used & (1 << (o - 1)) == 0
                || r.mod_fx[o as usize - 1] & (1 << k) == 0)
        {
            let m = if (o as usize) <= dkrt_abi::MAX_MODULES {
                o as u32 - 1
            } else {
                0
            };
            *out = TableFault {
                module: m,
                a: 0x400 | k as u32,
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
            if r.mod_fx[i] & (1 << k) != 0
                && (k >= MAX_FX || used & (1 << i) == 0 || r.fx_owner[k] != i as u8 + 1)
            {
                let o = if k < MAX_FX {
                    r.fx_owner[k] as u32
                } else {
                    0xff
                };
                *out = TableFault {
                    module: i as u32,
                    a: 0x400 | k as u32,
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

/// The module record behind a fault site: only the effect sites name a row of the effect table (by its byte offset), a voice site is the machines service's.
pub(crate) unsafe fn attribute<C: CoreApi>(core: &C, site: u32, id: u32) -> Option<usize> {
    if site != SITE_VOICE {
        module_of_fx_table_row(core, id / ROW_BYTES)
    } else {
        None
    }
}

/// Words of the diagnostics block that the effect guard of `core/fx` writes (`dkrt_sdk::fault`): effect faults (16 bits) | row of the last fault (byte offset + 1) << 16,
/// and the effect row switched off last (byte offset + 1).
const D_FXFAULT: usize = 28;
const D_FXDIS: usize = 31;

/// What `diag` has logged already: the fault count and the row switched off last.
#[repr(C)]
pub struct FxGuard {
    pub faults_seen: u32,
    pub disabled_seen: u32,
}

#[no_mangle]
pub static mut DKRT_FXGUARD: FxGuard = FxGuard {
    faults_seen: 0,
    disabled_seen: 0,
};

/// Forget what `diag` has seen (host tests, between two cores).
pub fn guard_reset() {
    unsafe {
        *addr_of_mut!(DKRT_FXGUARD) = FxGuard {
            faults_seen: 0,
            disabled_seen: 0,
        }
    }
}

/// The effect guard's words: a new fault is an `FX_FAULT`, a row switched off an `FX_DISABLED`, each with the source of the module that owns the row. The poll is a
/// second apart, so a burst is one event whose fault count says how many.
pub(crate) unsafe fn diag<C: CoreApi>(core: &C, buf: &[u32; DIAG_WORDS]) {
    let g = &mut *addr_of_mut!(DKRT_FXGUARD);
    let faults = buf[D_FXFAULT] & 0xffff;
    if faults != g.faults_seen {
        g.faults_seen = faults;
        let off = (buf[D_FXFAULT] >> 16).wrapping_sub(1);
        fx_event(core, logk::FX_FAULT, off / ROW_BYTES, faults);
    }
    if buf[D_FXDIS] != g.disabled_seen {
        g.disabled_seen = buf[D_FXDIS];
        if buf[D_FXDIS] != 0 {
            fx_event(
                core,
                logk::FX_DISABLED,
                (buf[D_FXDIS] - 1) / ROW_BYTES,
                faults,
            );
        }
    }
}

unsafe fn fx_event<C: CoreApi>(core: &C, kind: u16, row: u32, faults: u32) {
    let src = match module_of_fx_table_row(core, row) {
        Some(m) => (m as u16 + 1) << logk::SOURCE_SHIFT,
        None => 0,
    };
    core.log(kind, logk::F_ERROR | src, [row, faults, 0, 0, 0]);
}
