//! Filters of run-time modules (docs/plans/extension-model.md E7, docs/findings/148-filter-machines.md): the `FILTER` record, its rule, the sticky numbers 16 to 31 and the
//! table of the filters that are loaded. A filter has a long and a short name and at most two own knobs: knob 1 of a module filter is always the stock cutoff, so the
//! module owns knobs 2 and 3 (the stock FLTR1 page has three type-specific fields; field 4 is the cutoff).
//!
//! The service's own state is `DKRT_FILTERS`; what the ColdFire hooks read is `DKS_FLT` (below): the descriptor rows the stock FLTR page reads through its accessor, the
//! names, the answers of the parameter-code function and the mask of loaded rows. The hooks are in `dks/src/hooks-*.s`; the graph hook passes the filter row’s callback and control codes to the image adapter.
//!
//! The descriptor row of a module filter is a copy of the stock row of type 0 (Multi-Mode: the service reads it through `STOCK_ROW0`) with the names of the module, field
//! 4 (knob 1) = the stock cutoff code of that row and fields 5 and 6 = the parameter rows of the module's knobs 2 and 3. Those rows are ordinary run-time parameter rows
//! (`write_fx_param_row`: the template and the mirror index of the stock knob at the same field, so the value sits in the word the stock knob uses).
//!
//! Record body (big endian words, `LEN_FILTER` less the 8-byte header = 184 bytes): the key (`hash24 << 8`), the number of own knobs (0 to 2), the name and the short
//! name (24 bytes each), then two knob slots of 64 bytes: glyph, minimum, maximum, default (u32, raw = unit * 256), name (32 bytes), short name (16 bytes). A slot
//! that is not used is all zero. A `FILTER_V2` body appends one ColdFire image-relative draw callback offset.
//!
//! The numbers: a second sticky range (`dkrt_sdk::sticky`) of 16 numbers with its own settings key and map of 16 entries, so that every number can be remembered. The range
//! is a constant: it is the same on both devices (finding 148) and no stock table holds a filter type of 16 or more.
use crate::validate::text_ok;
use core::ptr::addr_of_mut;
use dkrt_abi::core_api::{CoreApi, LOG_INFO};
use dkrt_abi::mem::{iw, rd32, FwMem};
use dkrt_abi::registry::each;
use dkrt_abi::service::{Problem, SettleCtx};
use dkrt_sdk::records::{KIND_FILTER, KIND_FILTER_V2, LEN_FILTER, LEN_FILTER_V2};
use dkrt_sdk::sticky::{assign, clean, Event, Range};
use dkrt_sdk::*;
use dkrt_svc_params::dynreg::w;

/// The filter types of run-time modules.
pub const FIRST: u32 = 16;
pub const LAST: u32 = 31;
pub const ROWS: usize = (LAST - FIRST + 1) as usize;
/// The filters of one module (the keys one `CoreApi::keys_of` call gives).
pub const PER_MODULE: usize = 8;
/// The own knobs of a filter page: knobs 2 and 3 (knob 1 is the stock cutoff).
pub const OWN_KNOBS: usize = 2;
/// The settings key of the map of sticky numbers: `ROWS` words, big endian.
pub const SETTING: &[u8] = b"machines.filters";
/// Filter keys a boot plans (more get no number and are refused at their commit).
const MAX_KEYS: usize = 32;

const TEXT: usize = 24;
const BODY: usize = LEN_FILTER as usize - 8;
const BODY_V2: usize = LEN_FILTER_V2 as usize - 8;
const NAME: usize = 8;
const SHORT: usize = 8 + TEXT;
const KNOB0: usize = 8 + 2 * TEXT;
const KNOB_LEN: usize = 64;

const RANGE: Range = Range {
    lo: FIRST,
    hi: LAST,
};

pub mod what {
    pub const COUNT: &str = "more filters than one module may have";
    pub const SIZE: &str = "the body has the wrong size";
    pub const FILTER: &str = "a filter has a key without a hash, more than two knobs, or a name or knob text that is not printable ASCII, a knob with minimum above maximum, or a used slot beyond its knobs";
    pub const DRAW: &str = "the draw callback offset is not word-aligned";
    pub const KEY_TWICE: &str = "same filter key as an earlier filter";
}

// ---- the rule ------------------------------------------------------------------------------------------------------------------

/// A knob slot that is not used is all zero.
unsafe fn zero(p: *const u8, n: usize) -> bool {
    let mut i = 0;
    while i < n {
        if *p.add(i) != 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// The fields of a FILTER body of `BODY` bytes (the key's hash, the knob count, the texts, the knob slots).
pub unsafe fn body_ok(b: *const u8) -> bool {
    let key = rd32(b);
    let n = rd32(b.add(4)) as usize;
    if key >> 8 == 0 || key & 0xff != 0 || n > OWN_KNOBS {
        return false;
    }
    if !text_ok(b.add(NAME), TEXT) || !text_ok(b.add(SHORT), TEXT) {
        return false;
    }
    let mut j = 0;
    while j < OWN_KNOBS {
        let k = b.add(KNOB0 + KNOB_LEN * j);
        if j < n {
            if (rd32(k.add(4)) as i32) > (rd32(k.add(8)) as i32)
                || !text_ok(k.add(16), 32)
                || !text_ok(k.add(48), 16)
            {
                return false;
            }
        } else if !zero(k, KNOB_LEN) {
            return false;
        }
        j += 1;
    }
    true
}

pub fn is_kind(kind: u32) -> bool {
    kind == KIND_FILTER || kind == KIND_FILTER_V2
}

/// Check both filter record versions together so count and key uniqueness cover the whole module.
pub fn validate<'a>(
    records: impl IntoIterator<Item = (u32, &'a [u8])>,
    report: &mut dyn FnMut(Problem),
) {
    let mut keys = [0u32; PER_MODULE];
    for (i, (kind, body)) in records.into_iter().enumerate() {
        if i == PER_MODULE {
            report(Problem::set(KIND_FILTER, E_FORMAT, what::COUNT));
        }
        let size = if kind == KIND_FILTER_V2 {
            BODY_V2
        } else {
            BODY
        };
        if body.len() != size {
            report(Problem::record(kind, i, E_FORMAT, what::SIZE));
            continue;
        }
        // SAFETY: the length has been checked for this record version
        unsafe {
            if !body_ok(body.as_ptr()) {
                report(Problem::record(kind, i, E_FORMAT, what::FILTER));
            }
            if kind == KIND_FILTER_V2 && rd32(body.as_ptr().add(BODY)) & 1 != 0 {
                report(Problem::record(kind, i, E_FORMAT, what::DRAW));
            }
            let key = rd32(body.as_ptr());
            let mut with = 0;
            while with < i && with < PER_MODULE {
                if keys[with] == key {
                    report(Problem::clash(kind, i, with, E_FORMAT, what::KEY_TWICE));
                    break;
                }
                with += 1;
            }
            if i < PER_MODULE {
                keys[i] = key;
            }
        }
    }
}

// ---- the table of loaded filters -------------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Knob {
    pub glyph: u32,
    /// raw = unit * 256
    pub min: i32,
    pub max: i32,
    pub def: i32,
    pub name: [u8; 32],
    pub short: [u8; 16],
}

#[derive(Clone, Copy)]
pub struct Row {
    /// the module record that owns the row (index + 1, 0 free)
    pub owner: u8,
    pub knobs: u8,
    pub key: u32,
    pub name: [u8; TEXT],
    pub short: [u8; TEXT],
    pub knob: [Knob; OWN_KNOBS],
    /// the parameter code of each own knob (0: no knob or no registry)
    pub code: [u32; OWN_KNOBS],
    /// Relocated ColdFire graph callback, or 0 when this filter has none.
    pub draw: u32,
}

/// The loaded filters, row `t - FIRST` for filter type `t` (zero at the start: a `.bss` of the service file).
#[no_mangle]
pub static mut DKRT_FILTERS: [Row; ROWS] = unsafe { core::mem::zeroed() };

pub fn rows() -> &'static mut [Row; ROWS] {
    unsafe { &mut *addr_of_mut!(DKRT_FILTERS) }
}

unsafe fn text_at<const N: usize>(b: *const u8, at: usize) -> [u8; N] {
    let mut out = [0u8; N];
    let mut i = 0;
    while i < N {
        out[i] = *b.add(at + i);
        i += 1;
    }
    out
}

/// The row a checked FILTER body describes, owned by module record `owner`.
unsafe fn row_of(owner: u8, b: *const u8) -> Row {
    let mut r: Row = core::mem::zeroed();
    r.owner = owner;
    r.key = rd32(b);
    r.knobs = rd32(b.add(4)) as u8;
    r.name = text_at::<TEXT>(b, NAME);
    r.short = text_at::<TEXT>(b, SHORT);
    let mut j = 0;
    while j < OWN_KNOBS {
        let k = b.add(KNOB0 + KNOB_LEN * j);
        r.knob[j].glyph = rd32(k);
        r.knob[j].min = rd32(k.add(4)) as i32;
        r.knob[j].max = rd32(k.add(8)) as i32;
        r.knob[j].def = rd32(k.add(12)) as i32;
        r.knob[j].name = text_at::<32>(k, 16);
        r.knob[j].short = text_at::<16>(k, 48);
        j += 1;
    }
    r
}

/// Filter rows in use (the loaded filters).
pub fn used() -> u32 {
    let mut n = 0;
    let t = rows();
    let mut i = 0;
    while i < ROWS {
        if t[i].owner != 0 {
            n += 1;
        }
        i += 1;
    }
    n
}

/// The filter types module record `owner` (1 ..) filled, in type order, into `out`; how many.
pub fn types_of(owner: u8, out: &mut [u8; PER_MODULE]) -> usize {
    let t = rows();
    let mut n = 0;
    let mut i = 0;
    while i < ROWS {
        if t[i].owner == owner && n < PER_MODULE {
            out[n] = (FIRST as usize + i) as u8;
            n += 1;
        }
        i += 1;
    }
    n
}

/// Free the rows of module record `owner` (`Service::remove`).
pub fn remove(owner: u8) {
    let t = rows();
    let mut i = 0;
    while i < ROWS {
        if t[i].owner == owner {
            t[i] = unsafe { core::mem::zeroed() };
            unretire(i);
        }
        i += 1;
    }
}

/// The check of the filter records of a section (the RECORDS step): the rule, a number for every key, no number taken by another module or twice in the file.
pub unsafe fn check<C: CoreApi>(core: &C, reg: *const u8, image_size: u32) -> i32 {
    let mut bad = OK;
    let mut seen = [0u32; PER_MODULE];
    let mut n = 0usize;
    each(reg, |kind, b| {
        if !is_kind(kind) || bad != OK {
            return;
        }
        if n >= PER_MODULE || !body_ok(b) {
            bad = E_FORMAT;
            return;
        }
        if kind == KIND_FILTER_V2 {
            let draw = rd32(b.add(BODY));
            if draw >= image_size || draw & 1 != 0 {
                bad = E_FORMAT;
                return;
            }
        }
        let t = resolve(core, rd32(b));
        if t == 0 {
            bad = E_TYPE_USED; // the 16 numbers are all in use or referenced
            return;
        }
        let mut j = 0;
        while j < n {
            if seen[j] == t {
                bad = E_FORMAT;
                return;
            }
            j += 1;
        }
        seen[n] = t;
        n += 1;
        if rows()[(t - FIRST) as usize].owner != 0 {
            bad = E_TYPE_USED;
        }
    });
    bad
}

/// Fill the rows of the (checked) filter records of module record `owner`: the service's row, the parameter rows of its knobs and the hook table.
pub unsafe fn apply<C: CoreApi>(core: &C, owner: u8, reg: *const u8, base: u32) {
    let inf = dkrt_svc_params::dynreg(core);
    each(reg, |kind, b| {
        if !is_kind(kind) {
            return;
        }
        let t = resolve(core, rd32(b));
        if t >= FIRST && t <= LAST {
            let i = (t - FIRST) as usize;
            let mut row = row_of(owner, b);
            if kind == KIND_FILTER_V2 {
                row.draw = base + rd32(b.add(BODY));
            }
            rows()[i] = row;
            let mut j = 0;
            while j < rows()[i].knobs as usize {
                rows()[i].code[j] = knob_row(core, inf, owner, i, j);
                j += 1;
            }
            publish(core, inf, i);
        }
    });
}

/// The number of own knobs of the filter records of a section: each takes a run-time parameter row.
pub unsafe fn knob_count(reg: *const u8) -> u32 {
    let mut n = 0;
    each(reg, |kind, b| {
        if is_kind(kind) {
            n += rd32(b.add(4)).min(OWN_KNOBS as u32);
        }
    });
    n
}

// ---- the table the hooks read --------------------------------------------------------------------------------------------------

/// Words of a descriptor row (0x2c bytes): the long and short name (string characters), nine parameter codes (fields 0 to 8; fields 4 to 6 are the knobs of the FLTR1 page).
pub const DESC_WORDS: usize = 11;
/// The first knob word of a descriptor row (field 4) and the stock cutoff among them.
pub const KNOB_WORD: usize = 6;
/// Knob requests of the parameter-code function: three consecutive mirror indexes.
const REQUESTS: usize = 3;
/// The size of a string representation of the firmware (`make_rep`).
const REP: usize = 48;

/// What the hooks of the machines service read, one layout for both devices (`F_*` of `hooks-common.s`, `ASM_OFFSETS`): bit `i` of `mask` is set while filter type
/// `FIRST + i` is loaded; `base` is the first of the three request codes of the parameter-code function; `slots[i][r]` the code that function answers for request `r`
/// and filter `i`; `name[i]` the address of the long name; `rows[i]` the descriptor row the accessor returns.
#[repr(C, align(4))]
pub struct Tab {
    pub mask: u32,
    pub base: u32,
    pub slots: [[u32; REQUESTS]; ROWS],
    pub name: [u32; ROWS],
    pub rows: [[u32; DESC_WORDS]; ROWS],
}

#[no_mangle]
pub static mut DKS_FLT: Tab = unsafe { core::mem::zeroed() };

/// The byte offsets the assembler files name (`.equ F_NAME, offset`), for the test that compares them with `Tab`.
pub const ASM_OFFSETS: &[(&str, usize)] = &[
    ("F_MASK", core::mem::offset_of!(Tab, mask)),
    ("F_BASE", core::mem::offset_of!(Tab, base)),
    ("F_SLOTS", core::mem::offset_of!(Tab, slots)),
    ("F_NAMES", core::mem::offset_of!(Tab, name)),
    ("F_ROWS", core::mem::offset_of!(Tab, rows)),
    ("F_ROW_BYTES", 4 * DESC_WORDS),
];

pub fn tab() -> &'static mut Tab {
    unsafe { &mut *addr_of_mut!(DKS_FLT) }
}

#[repr(C, align(4))]
struct Rep([u8; REP]);

/// The string representations of the long and short name of each row.
static mut REPS: [[Rep; 2]; ROWS] = unsafe { core::mem::zeroed() };

/// The address of the stock descriptor row of filter type 0 (Multi-Mode), 0 until `set_stock` gave it: the accessor of the firmware answers it (`machines.dks` asks at
/// `setup`). The module rows copy its words.
static mut STOCK_ROW0: u32 = 0;

/// Give the stock row of filter type 0 (the service file's `setup`; the tests give a fake one).
pub fn set_stock(row: u32) {
    unsafe { STOCK_ROW0 = row }
}

fn stock() -> u32 {
    unsafe { STOCK_ROW0 }
}

unsafe fn text_len(b: &[u8]) -> usize {
    let mut n = 0;
    while n < b.len() && b[n] != 0 {
        n += 1;
    }
    n
}

/// The mirror index of parameter row `code`.
unsafe fn mirror_of<C: CoreApi>(core: &C, inf: *const u32, code: u32) -> u32 {
    core.rd(iw(inf, w::P_TABLE) + code * iw(inf, w::PF_SIZE) + 4)
}

/// The parameter row of own knob `j` of row `i`: a free run-time row filled from the stock knob at the same field (its template and mirror index), the module's name,
/// range and default. 0 when there is no registry, no stock row or no free row (the commit checked the free rows).
unsafe fn knob_row<C: CoreApi>(core: &C, inf: *const u32, owner: u8, i: usize, j: usize) -> u32 {
    let tpl = if stock() != 0 && !inf.is_null() {
        core.rd(stock() + 4 * (KNOB_WORD + 1 + j) as u32)
    } else {
        0
    };
    if tpl == 0 {
        return 0;
    }
    let s = crate::records::rstate();
    let mut k = 0;
    while k < limits::PARAM_ROWS && (s.param_owner[k] != 0 || k as u32 >= iw(inf, w::P_DYN_N)) {
        k += 1;
    }
    if k >= limits::PARAM_ROWS || k as u32 >= iw(inf, w::P_DYN_N) {
        return 0;
    }
    s.param_owner[k] = owner;
    let q = &rows()[i].knob[j];
    let mut n = 0;
    while n < 32 {
        s.s.param_name[k][n] = q.name[n];
        n += 1;
    }
    n = 0;
    while n < 16 {
        s.s.param_short[k][n] = q.short[n];
        n += 1;
    }
    let code = iw(inf, w::P_DYN0) + k as u32;
    dkrt_svc_params::write_fx_param_row(
        core,
        inf,
        code,
        tpl,
        mirror_of(core, inf, tpl),
        q.min as u32,
        q.max as u32,
        q.def as u32,
        s.s.param_name[k].as_ptr() as u32,
        s.s.param_short[k].as_ptr() as u32,
    );
    code
}

/// Rows were published while the stock row was not filled (or not known): `refresh` completes them. The order of the firmware's initialiser (which fills the stock rows) and of
/// the services' apply is not established from the image, so the table is made robust instead: the rows keep their names and mask bit, the codes follow at the first `tick` after
/// the stock row has its words.
static mut STALE: bool = false;

/// Does the stock row of type 0 hold its words (the cutoff code in field 4 is not 0)?
unsafe fn stock_ready<C: CoreApi>(core: &C) -> bool {
    stock() != 0 && core.rd(stock() + 4 * KNOB_WORD as u32) != 0
}

/// Complete the rows that were published before the stock row was filled: their knob parameter rows, descriptor words and requests (`Service::tick`). A flag test when
/// nothing is stale.
pub unsafe fn refresh<C: CoreApi>(core: &C) {
    if !STALE || !stock_ready(core) {
        return;
    }
    let inf = dkrt_svc_params::dynreg(core);
    STALE = false;
    let mut i = 0;
    while i < ROWS {
        let (owner, knobs) = (rows()[i].owner, rows()[i].knobs as usize);
        if owner != 0 {
            let mut j = 0;
            while j < knobs {
                if rows()[i].code[j] == 0 {
                    rows()[i].code[j] = knob_row(core, inf, owner, i, j);
                }
                j += 1;
            }
            publish(core, inf, i);
        }
        i += 1;
    }
}

/// The hook table of row `i`: the descriptor row, the name, the answers of the parameter-code function, then the mask bit.
unsafe fn publish<C: CoreApi>(core: &C, inf: *const u32, i: usize) {
    let r = &rows()[i];
    let t = tab();
    t.name[i] = r.name.as_ptr() as u32;
    if !stock_ready(core) {
        STALE = true;
    }
    let st = stock();
    let mut x = 0;
    while x < DESC_WORDS {
        t.rows[i][x] = if st != 0 {
            core.rd(st + 4 * x as u32)
        } else {
            0
        };
        x += 1;
    }
    let rp = &mut REPS[i];
    t.rows[i][0] = dkrt_svc_params::make_rep(rp[0].0.as_mut_ptr(), r.name.as_ptr(), false);
    t.rows[i][1] = dkrt_svc_params::make_rep(rp[1].0.as_mut_ptr(), r.short.as_ptr(), false);
    let mut j = 0;
    while j < OWN_KNOBS {
        t.rows[i][KNOB_WORD + 1 + j] = if j < r.knobs as usize { r.code[j] } else { 0 };
        j += 1;
    }
    if st != 0 && !inf.is_null() {
        // the request that reaches each knob field is the mirror index of the stock knob there; the first of the three is the base
        let mut lo = u32::MAX;
        let mut f = 0;
        while f < REQUESTS {
            let m = mirror_of(core, inf, core.rd(st + 4 * (KNOB_WORD + f) as u32));
            lo = lo.min(m);
            f += 1;
        }
        t.base = lo;
        f = 0;
        while f < REQUESTS {
            let m = mirror_of(core, inf, core.rd(st + 4 * (KNOB_WORD + f) as u32));
            let at = m.wrapping_sub(lo) as usize;
            if at < REQUESTS {
                t.slots[i][at] = t.rows[i][KNOB_WORD + f];
            }
            f += 1;
        }
    }
    t.mask |= 1 << i;
}

fn unretire(i: usize) {
    let t = tab();
    t.mask &= !(1 << i);
    t.name[i] = 0;
    t.slots[i] = [0; REQUESTS];
    t.rows[i] = [0; DESC_WORDS];
}

/// The chooser's order vector `vec` (begin, end, capacity words) with the loaded module filters appended, in a buffer of `alloc` (the firmware's operator new) that
/// replaces the stock one, which `free` gives back. The vector is the caller's own copy, which the stock function allocates anew on every call and the list builder frees
/// after its loop (finding 158); the stock function's static vector is never touched, so a second call gives six plus the loaded filters again. A failed allocation leaves the stock vector. Returns the number appended.
pub unsafe fn extend_list<M: FwMem>(
    m: &M,
    vec: u32,
    alloc: &mut dyn FnMut(u32) -> u32,
    free: &mut dyn FnMut(u32),
) -> u32 {
    let mask = tab().mask;
    let k = mask.count_ones();
    let (begin, end) = (m.rd(vec), m.rd(vec + 4));
    if k == 0 || end < begin {
        return 0;
    }
    let n = (end - begin) / 4;
    let buf = alloc(4 * (n + k));
    if buf == 0 {
        return 0;
    }
    let mut x = 0;
    while x < n {
        m.wr(buf + 4 * x, m.rd(begin + 4 * x));
        x += 1;
    }
    let mut at = n;
    let mut i = 0;
    while i < ROWS {
        if mask & (1 << i) != 0 {
            m.wr(buf + 4 * at, FIRST + i as u32);
            at += 1;
        }
        i += 1;
    }
    free(begin);
    m.wr(vec, buf);
    m.wr(vec + 4, buf + 4 * at);
    m.wr(vec + 8, buf + 4 * at);
    k
}

// ---- the sticky numbers ----------------------------------------------------------------------------------------------------------

/// The boot's assignment, peekable by the proof tools (`DKRT_ASSIGN_FILTERS`): `magic` "DKAF", `n` filters planned, `dirty` 1 when the map changed.
#[repr(C, align(4))]
pub struct State {
    magic: u32,
    n: usize,
    dirty: u32,
    map: [u32; ROWS],
    /// Per planned filter: the selection index of its module, its key, its number (0: none was free).
    idx: [u32; MAX_KEYS],
    key: [u32; MAX_KEYS],
    ty: [u32; MAX_KEYS],
}

#[no_mangle]
pub static mut DKRT_ASSIGN_FILTERS: State = State {
    magic: 0,
    n: 0,
    dirty: 0,
    map: [0; ROWS],
    idx: [0; MAX_KEYS],
    key: [0; MAX_KEYS],
    ty: [0; MAX_KEYS],
};

fn st() -> &'static mut State {
    unsafe { &mut *addr_of_mut!(DKRT_ASSIGN_FILTERS) }
}

/// Entries of the sticky map in use now.
pub fn map_used() -> u32 {
    let mut n = 0;
    let mut i = 0;
    while i < ROWS {
        if st().map[i] != 0 {
            n += 1;
        }
        i += 1;
    }
    n
}

unsafe fn stored<C: CoreApi>(core: &C) -> [u32; ROWS] {
    let mut raw = [0u8; ROWS * 4];
    let mut map = [0u32; ROWS];
    if core.settings_get(SETTING, &mut raw) == raw.len() as i32 {
        let mut i = 0;
        while i < ROWS {
            map[i] =
                u32::from_be_bytes([raw[4 * i], raw[4 * i + 1], raw[4 * i + 2], raw[4 * i + 3]]);
            i += 1;
        }
    }
    map
}

unsafe fn save<C: CoreApi>(core: &C, map: &[u32; ROWS]) -> bool {
    let mut raw = [0u8; ROWS * 4];
    let mut i = 0;
    while i < ROWS {
        raw[4 * i..4 * i + 4].copy_from_slice(&map[i].to_be_bytes());
        i += 1;
    }
    core.settings_put(SETTING, &raw) == OK
}

/// Assign the numbers of every filter of the selected modules (`Service::settle`), by the rules of the machines' assignment (`assign.rs`) over the filter range.
pub unsafe fn plan<C: CoreApi>(core: &C, sel: &SettleCtx) {
    let a = st();
    a.magic = 0x444b_4146; // "DKAF"
    a.n = 0;
    let from = stored(core);
    clean(RANGE, &from, &mut a.map);
    let mut k = 0;
    while k < sel.count {
        let mut keys = [0u32; 8];
        let n = core.keys_of(k as u32, KIND_FILTER, &mut keys);
        let mut i = 0;
        while i < n && a.n < MAX_KEYS {
            a.idx[a.n] = k as u32;
            a.key[a.n] = keys[i];
            a.ty[a.n] = 0;
            a.n += 1;
            i += 1;
        }
        k += 1;
    }
    let n = a.n;
    a.dirty = assign(
        RANGE,
        &from,
        &mut a.map,
        &a.key,
        &mut a.ty,
        n,
        &mut |ev: &Event| {
            let flags = if ev.error { logk::F_ERROR } else { LOG_INFO };
            core.log(ev.kind, flags, ev.args);
        },
    );
    if a.dirty != 0 {
        a.dirty = !save(core, &a.map) as u32; // a refused write stays dirty: the next boot assigns the same numbers and tries again
    }
}

/// The filter type for key `key` of the module being loaded (`CoreApi::boot_index`), 0 when `plan` gave none.
pub unsafe fn resolve<C: CoreApi>(core: &C, key: u32) -> u32 {
    let a = st();
    let at = core.boot_index();
    let mut i = 0;
    while i < a.n {
        if a.key[i] == key && a.idx[i] == at {
            return a.ty[i];
        }
        i += 1;
    }
    0
}
