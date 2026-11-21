//! The builder calls of the machines (docs/plans/sdk.md 6.2) and their commit (6.3), as the machines service has them: the calls that name a machine, its pages and
//! their parameters (`dk_module_ctx_add_machine`, `dk_machine_add_page`, `dk_page_add_param`, `dk_param_*`, `dk_page_canvas`, `dk_page_bind*`) are the service's
//! exports (`dks/src/builder.rs`), and the state they fill is a static of this service: one module is in `init` at a time.
//!
//! `module_begin` opens the state for module record `rec`, every call appends to it, and `module_commit` checks it and lowers it to the records of the `register`
//! section (kinds PARAM, MACHINE, PAGE, ACTION and native page v1/v2, in this order) in two passes over the loader's `RecordSink` (`PREPARE` measures and checks,
//! `WRITE` writes and declares the voice rows). The loader gives the block to the registry walker, which calls this service's `check` and `apply`.
//!
//! A call never fails one by one: on overflow, on a bad argument, outside `init` or with a handle that is not ours it returns the placeholder handle (`placeholder`), the
//! first error is kept, and the setters ignore a placeholder. The error is reported at the commit. A handle is the address of an element of the state.
//!
//! Device facts (knob positions of a machine's words, stock rows by glyph, clone_of, level) come from `Profile`, read from the `dynreg_info` table.
use crate::{assign, dsp};
use core::ptr::{addr_of, addr_of_mut};
use dkrt_abi::core_api::{CoreApi, LOG_INFO};
use dkrt_abi::keys::KeysCopy;
use dkrt_abi::mem::iw;
use dkrt_abi::service::{commit_phase, ModuleCommitCtx, RecordSink};
use dkrt_sdk::records::{
    KIND_ACTION, KIND_FILTER, KIND_FILTER_V2, KIND_MACHINE, KIND_NATIVE_PAGE, KIND_NATIVE_PAGE_V2,
    KIND_PAGE, KIND_PARAM, LEN_ACTION, LEN_FILTER, LEN_FILTER_V2, LEN_MACHINE, LEN_NATIVE_PAGE,
    LEN_NATIVE_PAGE_V2, LEN_PAGE, LEN_PARAM,
};
use dkrt_sdk::*;
use dkrt_svc_params::builder::{fail, find, len_of, put_text, room, Gate};
use dkrt_svc_params::dynreg::w;

/// The most machines, pages and records (machines, pages, parameters and click actions together) of one module.
pub const MAX_M: usize = limits::MACHINE_ROWS;
pub const MAX_PG: usize = 16;
pub const MAX_PA: usize = dkneeds::MAX_RECORDS as usize;
const TEXT: usize = 24;
const KNOBS: usize = limits::PAGE_KNOBS;
const KNOB_PARAM: u32 = 0x8000_0000;
/// The `owner` of a page of filter `f` is `FILTER_OWNER + f`; a machine's pages have its index (below `MAX_M`), so no machine loop meets a filter page.
const FILTER_OWNER: u8 = 0x80;
const MAX_F: usize = crate::filter::PER_MODULE;
const POS_END: u32 = 255;

/// The device facts of the lowering (a copy of the words of `dynreg_info` from 65; see core/dynreg).
#[derive(Clone, Copy)]
pub struct Profile {
    pub device: u32,
    /// The device keeps a machine's knobs in page rows (Digitone II).
    pub pages: bool,
    pub slot_base: u32,
    pub clone_of: u32,
    pub level: u32,
    pub own_dsp: bool,
    pub tune_code: u32,
    pub tune_slot: u32,
    pub level_code: u32,
    pub level_knob: u32,
    pub nslots: u32,
    pub slots: [u32; 8],
    /// Stock rows by glyph: encoder, fader, pan, onoff, tune, play.
    pub tpl: [u32; 6],
}

impl Profile {
    /// The facts of `dynreg_info` version 3 (`inf`).
    pub unsafe fn read(inf: *const u32) -> Profile {
        let mut p = Profile {
            device: iw(inf, w::DEVICE),
            pages: iw(inf, w::PG_ROWS) != 0,
            slot_base: iw(inf, w::SLOT_BASE),
            clone_of: iw(inf, w::P_CLONE),
            level: iw(inf, w::P_LEVEL),
            own_dsp: iw(inf, w::P_OWN) != 0,
            tune_code: iw(inf, w::P_TUNE),
            tune_slot: iw(inf, w::P_TUNE_SLOT),
            level_code: iw(inf, w::P_LEVEL_CODE),
            level_knob: iw(inf, w::P_LEVEL_KNOB),
            nslots: iw(inf, w::P_NSLOTS).min(8),
            slots: [0; 8],
            tpl: [0; 6],
        };
        let mut i = 0;
        while i < 8 {
            p.slots[i] = iw(inf, w::P_SLOTS + i);
            i += 1;
        }
        i = 0;
        while i < 6 {
            p.tpl[i] = iw(inf, w::P_TPL + i);
            i += 1;
        }
        p
    }
}

#[derive(Clone, Copy)]
pub struct CMachine {
    /// the machine's index in the module (`DK_MACHINE_ID_<ID>`): the position of its key in the keys table
    pub index: u32,
    pub name: [u8; TEXT],
    pub short: [u8; TEXT],
}
#[derive(Clone, Copy)]
pub struct CFilter {
    /// the filter's index in the module (`DK_FILTER_ID_<ID>`): the position of its key in the keys table
    pub index: u32,
    /// the key, read from the keys table at the commit
    pub key: u32,
    pub name: [u8; TEXT],
    pub short: [u8; TEXT],
    pub has_page: bool,
}
#[derive(Clone, Copy)]
pub struct CPage {
    pub owner: u8,
    pub name: [u8; TEXT],
    /// the tab label of the page record (`short_name` of `dk_machine_add_page`)
    pub short: [u8; TEXT],
    pub canvas: bool,
    pub flags: u32,
    pub draw: u32,
    pub event: u32,
    pub arg: u32,
    pub nbind: u8,
    /// (kind 1 param / 2 local, collection param index or local id, encoder slot)
    pub binds: [[u32; 3]; 8],
    pub nparam: u8,
}
#[derive(Clone, Copy)]
pub struct CParam {
    pub page: u8,
    pub name: [u8; 32],
    pub short: [u8; 16],
    /// raw = unit * 256
    pub min: i32,
    pub max: i32,
    pub def: i32,
    pub glyph: u32,
    pub latched: bool,
    pub click: u32,
    pub click_arg: u32,
}

const M0: CMachine = CMachine {
    index: 0,
    name: [0; TEXT],
    short: [0; TEXT],
};
const F0: CFilter = CFilter {
    index: 0,
    key: 0,
    name: [0; TEXT],
    short: [0; TEXT],
    has_page: false,
};
const G0: CPage = CPage {
    owner: 0,
    name: [0; TEXT],
    short: [0; TEXT],
    canvas: false,
    flags: 0,
    draw: 0,
    event: 0,
    arg: 0,
    nbind: 0,
    binds: [[0; 3]; 8],
    nparam: 0,
};
const P0: CParam = CParam {
    page: 0,
    name: [0; 32],
    short: [0; 16],
    min: 0,
    max: 127 * 256,
    def: 0,
    glyph: 0,
    latched: false,
    click: 0,
    click_arg: 0,
};

/// What `init` declared of the machines.
pub struct Build {
    pub err: i32,
    pub machines: [CMachine; MAX_M],
    pub nm: usize,
    pub pages: [CPage; MAX_PG],
    pub np: usize,
    pub params: [CParam; MAX_PA],
    pub nparam: usize,
    pub filters: [CFilter; MAX_F],
    pub nf: usize,
    records: usize,
}

impl Build {
    pub const fn new() -> Build {
        Build {
            err: 0,
            machines: [M0; MAX_M],
            nm: 0,
            pages: [G0; MAX_PG],
            np: 0,
            params: [P0; MAX_PA],
            nparam: 0,
            filters: [F0; MAX_F],
            nf: 0,
            records: 0,
        }
    }

    /// Nothing was declared.
    pub fn is_empty(&self) -> bool {
        self.nm + self.np + self.nparam + self.nf == 0
    }

    /// The records of the collection so far (machines, pages, parameters, click actions).
    pub fn records(&self) -> usize {
        self.records
    }

    fn fail(&mut self, e: i32) {
        fail(&mut self.err, e);
    }

    /// One more record of the bounded collection; false (error kept) when full.
    fn room(&mut self) -> bool {
        room(&mut self.err, &mut self.records, MAX_PA)
    }

    pub fn add_machine(&mut self, index: u32, name: *const u8, short: *const u8) -> *mut u8 {
        if self.nm >= MAX_M || !self.room() {
            self.fail(E_DECL_FULL);
            return placeholder();
        }
        let i = self.nm;
        let mut m = M0;
        m.index = index;
        let mut e = 0;
        unsafe {
            put_text(&mut m.name, name, &mut e);
            put_text(&mut m.short, short, &mut e);
        }
        self.fail(e);
        self.machines[i] = m;
        self.nm += 1;
        &mut self.machines[i] as *mut CMachine as *mut u8
    }

    /// `dk_module_ctx_add_filter`: a filter of the module (a bounded array of its own: its handle is an element of it, so a machine handle is not one).
    pub fn add_filter(&mut self, index: u32, name: *const u8, short: *const u8) -> *mut u8 {
        if self.nf >= MAX_F || !self.room() {
            self.fail(E_DECL_FULL);
            return placeholder();
        }
        let i = self.nf;
        let mut f = F0;
        f.index = index;
        let mut e = 0;
        unsafe {
            put_text(&mut f.name, name, &mut e);
            put_text(&mut f.short, short, &mut e);
        }
        self.fail(e);
        self.filters[i] = f;
        self.nf += 1;
        &mut self.filters[i] as *mut CFilter as *mut u8
    }

    /// `dk_filter_add_page`: the filter's one page, which takes its name; a second page is `E_PAGE_FULL`, a handle that is not a filter of this collection (a
    /// machine's, for one) is `E_FORMAT`.
    pub fn filter_add_page(&mut self, f: *mut u8) -> *mut u8 {
        let owner = match find(&self.filters, f) {
            Some(i) => i,
            None => {
                if gate().foreign(f) {
                    self.fail(E_FORMAT);
                }
                return placeholder();
            }
        };
        if self.filters[owner].has_page {
            self.fail(E_PAGE_FULL);
            return placeholder();
        }
        if self.np >= MAX_PG || !self.room() {
            self.fail(E_DECL_FULL);
            return placeholder();
        }
        let i = self.np;
        let mut g = G0;
        g.owner = FILTER_OWNER + owner as u8;
        g.name = self.filters[owner].name;
        g.short = self.filters[owner].short;
        self.filters[owner].has_page = true;
        self.pages[i] = g;
        self.np += 1;
        &mut self.pages[i] as *mut CPage as *mut u8
    }

    pub fn machine_add_page(&mut self, m: *mut u8, name: *const u8, short: *const u8) -> *mut u8 {
        let owner = match find(&self.machines, m) {
            Some(i) => i,
            None => {
                if find(&self.filters, m).is_some() {
                    self.fail(E_FORMAT); // a filter handle is not a machine
                }
                return placeholder();
            }
        };
        if self.np >= MAX_PG || !self.room() {
            self.fail(E_DECL_FULL);
            return placeholder();
        }
        let i = self.np;
        let mut g = G0;
        g.owner = owner as u8;
        let mut e = 0;
        unsafe {
            put_text(&mut g.name, name, &mut e);
            put_text(&mut g.short, short, &mut e);
        }
        self.fail(e);
        self.pages[i] = g;
        self.np += 1;
        &mut self.pages[i] as *mut CPage as *mut u8
    }

    /// `dk_page_add_param`: `def` is in knob units. A handle of another service (an effect's page) is a machine parameter on an effect page: refused.
    pub fn page_add_param(
        &mut self,
        p: *mut u8,
        name: *const u8,
        short: *const u8,
        def: u32,
    ) -> *mut u8 {
        let pg = match find(&self.pages, p) {
            Some(i) => i,
            None => {
                if gate().foreign(p) {
                    self.fail(E_FORMAT);
                }
                return placeholder();
            }
        };
        // a filter page has the knobs 2 and 3 only: a third parameter is refused and not recorded
        let of_filter = self.pages[pg].owner >= FILTER_OWNER;
        if of_filter && self.pages[pg].nparam as usize >= crate::filter::OWN_KNOBS {
            self.fail(E_PAGE_FULL);
            return placeholder();
        }
        if self.nparam >= MAX_PA || !self.room() {
            self.fail(E_DECL_FULL);
            return placeholder();
        }
        let i = self.nparam;
        let mut q = P0;
        q.page = pg as u8;
        q.def = (def as i32).wrapping_mul(256);
        let mut e = 0;
        unsafe {
            put_text(&mut q.name, name, &mut e);
            put_text(&mut q.short, short, &mut e);
        }
        self.fail(e);
        if self.pages[pg].nparam as usize >= KNOBS {
            self.fail(E_FORMAT);
        } else {
            self.pages[pg].nparam += 1;
        }
        self.params[i] = q;
        self.nparam += 1;
        &mut self.params[i] as *mut CParam as *mut u8
    }

    pub fn param_range(&mut self, h: *mut u8, min: u32, max: u32) {
        if let Some(i) = find(&self.params, h) {
            self.params[i].min = (min as i32).wrapping_mul(256);
            self.params[i].max = (max as i32).wrapping_mul(256);
        }
    }

    pub fn param_glyph(&mut self, h: *mut u8, glyph: u32) {
        if let Some(i) = find(&self.params, h) {
            self.params[i].glyph = glyph;
        }
    }

    pub fn param_latched(&mut self, h: *mut u8) {
        if let Some(i) = find(&self.params, h) {
            self.params[i].latched = true;
        }
    }

    /// A click action on an effect parameter (a handle of the fx service) is refused at the commit.
    pub fn param_on_click(&mut self, h: *mut u8, f: u32, arg: u32) {
        match find(&self.params, h) {
            Some(i) => {
                if self.pages[self.params[i].page as usize].owner >= FILTER_OWNER {
                    self.fail(E_FORMAT); // a filter has no click action
                    return;
                }
                if self.params[i].click == 0 && f != 0 && !self.room() {
                    return;
                }
                self.params[i].click = f;
                self.params[i].click_arg = arg;
            }
            None => {
                if gate().foreign(h) {
                    self.fail(E_FORMAT);
                }
            }
        }
    }

    pub fn page_canvas(&mut self, p: *mut u8, flags: u32, draw: u32, event: u32, arg: u32) {
        if let Some(i) = find(&self.pages, p) {
            if self.pages[i].owner >= FILTER_OWNER {
                if flags != 0 || draw == 0 || event != 0 || arg != 0 {
                    self.fail(E_FORMAT); // a filter graph takes only a draw callback
                } else {
                    self.pages[i].draw = draw;
                }
                return;
            }
            let g = &mut self.pages[i];
            g.canvas = true;
            g.flags = flags;
            g.draw = draw;
            g.event = event;
            g.arg = arg;
        }
    }

    fn bind(&mut self, p: *mut u8, kind: u32, value: u32, slot: u32) {
        if let Some(i) = find(&self.pages, p) {
            if self.pages[i].owner >= FILTER_OWNER {
                self.fail(E_FORMAT);
                return;
            }
            let g = &mut self.pages[i];
            if g.nbind as usize >= 8 || slot >= 8 {
                self.fail(E_FORMAT);
                return;
            }
            let n = g.nbind as usize;
            g.binds[n] = [kind, value, slot];
            g.nbind += 1;
        }
    }

    pub fn page_bind(&mut self, p: *mut u8, prm: *mut u8, slot: u32) {
        match find(&self.params, prm) {
            Some(i) => self.bind(p, 1, i as u32, slot),
            None => {
                if find(&self.pages, p).is_some() {
                    self.fail(E_FORMAT); // a param that is not ours (a placeholder after an earlier error: the error is kept already)
                }
            }
        }
    }

    pub fn page_bind_local(&mut self, p: *mut u8, local: u32, slot: u32) {
        self.bind(p, 2, local, slot);
    }
}

// ---- the open collection and the placeholder ------------------------------------------------------------------------------------

/// Zero at the start (a `.bss` of the service file, not 5 KiB of initialised data in it): `begin` writes `Build::new()` before the first builder call of every module.
static mut BUILD: Build = unsafe { core::mem::zeroed() };
static mut GATE: Gate = Gate::new();
/// The callback arguments of the ACTION and native page records `WRITE` wrote, for `apply` (`take_args`).
static mut ARGS: Option<ApplyArgs> = None;

fn gate() -> &'static mut Gate {
    unsafe { &mut *addr_of_mut!(GATE) }
}

/// The placeholder handle of a refused call.
pub fn placeholder() -> *mut u8 {
    gate().placeholder()
}

/// The collection (host tests read it).
pub fn build() -> &'static mut Build {
    unsafe { &mut *addr_of_mut!(BUILD) }
}

/// `module_begin`: forget what an earlier module collected and open the collection for module record `rec`.
pub unsafe fn begin(rec: usize) {
    core::ptr::write(addr_of_mut!(BUILD), Build::new());
    *addr_of_mut!(ARGS) = None;
    gate().begin(rec);
}

/// Close the collection: later builder calls return the placeholder.
pub unsafe fn end() {
    gate().end();
}

// the builder calls (exported by the service file: `dks/src/builder.rs`); a call that names a handle comes from the module whose `init` is open

pub unsafe fn add_machine<C: CoreApi>(
    core: &C,
    ctx: *mut u8,
    index: u32,
    name: *const u8,
    short: *const u8,
) -> *mut u8 {
    if gate().open_for(core, ctx) {
        build().add_machine(index, name, short)
    } else {
        placeholder()
    }
}
pub unsafe fn add_filter<C: CoreApi>(
    core: &C,
    ctx: *mut u8,
    index: u32,
    name: *const u8,
    short: *const u8,
) -> *mut u8 {
    if gate().open_for(core, ctx) {
        build().add_filter(index, name, short)
    } else {
        placeholder()
    }
}
pub unsafe fn filter_add_page<C: CoreApi>(core: &C, f: *mut u8) -> *mut u8 {
    if gate().open_any(core) {
        build().filter_add_page(f)
    } else {
        placeholder()
    }
}
pub unsafe fn machine_add_page<C: CoreApi>(
    core: &C,
    m: *mut u8,
    name: *const u8,
    short: *const u8,
) -> *mut u8 {
    if gate().open_any(core) {
        build().machine_add_page(m, name, short)
    } else {
        placeholder()
    }
}
pub unsafe fn page_add_param<C: CoreApi>(
    core: &C,
    p: *mut u8,
    name: *const u8,
    short: *const u8,
    def: u32,
) -> *mut u8 {
    if gate().open_any(core) {
        build().page_add_param(p, name, short, def)
    } else {
        placeholder()
    }
}
pub unsafe fn param_range<C: CoreApi>(core: &C, prm: *mut u8, min: u32, max: u32) {
    if gate().open_any(core) {
        build().param_range(prm, min, max);
    }
}
pub unsafe fn param_glyph<C: CoreApi>(core: &C, prm: *mut u8, glyph: u32) {
    if gate().open_any(core) {
        build().param_glyph(prm, glyph);
    }
}
pub unsafe fn param_latched<C: CoreApi>(core: &C, prm: *mut u8) {
    if gate().open_any(core) {
        build().param_latched(prm);
    }
}
pub unsafe fn param_on_click<C: CoreApi>(core: &C, prm: *mut u8, f: u32, arg: u32) {
    if gate().open_any(core) {
        build().param_on_click(prm, f, arg);
    }
}
pub unsafe fn page_canvas<C: CoreApi>(
    core: &C,
    p: *mut u8,
    flags: u32,
    draw: u32,
    event: u32,
    arg: u32,
) {
    if gate().open_any(core) {
        build().page_canvas(p, flags, draw, event, arg);
    }
}
pub unsafe fn page_bind<C: CoreApi>(core: &C, p: *mut u8, prm: *mut u8, slot: u32) {
    if gate().open_any(core) {
        build().page_bind(p, prm, slot);
    }
}
pub unsafe fn page_bind_local<C: CoreApi>(core: &C, p: *mut u8, local: u32, slot: u32) {
    if gate().open_any(core) {
        build().page_bind_local(p, local, slot);
    }
}

// ---- lowering to records -----------------------------------------------------------------------------------------------------

/// The (min, max) knob units a glyph allows (`typed.rs glyph_info`); None for an unknown glyph.
fn glyph_range(g: u32) -> Option<(i32, i32)> {
    if g == 0 || g == 1 || g == 2 {
        Some((0, 127))
    } else if g == 3 {
        Some((0, 1))
    } else if g == 4 {
        Some((4, 124))
    } else if g == 5 {
        Some((0, 3))
    } else {
        None
    }
}

/// How the collection lowers: per machine, the knob position of each of its words, and the file index of each collected machine param.
pub struct Plan {
    /// file index (PARAM record order) of collection param i
    pub file: [u8; MAX_PA],
    pub nfile: usize,
    /// collection param index of file param k
    pub of_file: [u8; MAX_PA],
    /// knob position of collection param i
    pub pos: [u8; MAX_PA],
    /// word of collection param i within its machine
    pub word: [u8; MAX_PA],
    /// the machine a collection param belongs to
    pub machine: [u8; MAX_PA],
}

impl Plan {
    pub const fn new() -> Plan {
        Plan {
            file: [u8::MAX; MAX_PA],
            nfile: 0,
            of_file: [0; MAX_PA],
            pos: [0; MAX_PA],
            word: [0; MAX_PA],
            machine: [0; MAX_PA],
        }
    }
}

/// The key of a machine of the collection: the keys table row at its index (`check_commit` refused an index outside the table).
pub fn machine_key(keys: &KeysCopy, m: &CMachine) -> u32 {
    let i = m.index as usize;
    if i < keys.nm {
        keys.mk[i]
    } else {
        0
    }
}

/// The knob page of machine `m`: the index of its only non-canvas page, None without one; Err when it has two.
fn knob_page(b: &Build, m: usize) -> Result<Option<usize>, i32> {
    let mut found = None;
    let mut i = 0;
    while i < b.np {
        let g = &b.pages[i];
        if g.owner as usize == m && !g.canvas {
            if found.is_some() {
                return Err(E_FORMAT); // a machine has one knob page for now
            }
            found = Some(i);
        }
        i += 1;
    }
    Ok(found)
}

/// Lower the collection into `o`: the records of PARAM, MACHINE, PAGE, ACTION and native page kinds. `o` without a buffer only measures. `base` turns callback addresses into image
/// offsets. `voice[m]`: the DSP section has a voice for machine `m` (only then may it render with its own DSP routine). The errors are refusals of the commit.
pub fn lower(
    b: &Build,
    p: &Profile,
    keys: &KeysCopy,
    voice: &[bool; MAX_M],
    base: u32,
    o: &mut RecordSink,
    plan: &mut Plan,
) -> Result<(), i32> {
    if b.err != 0 {
        return Err(b.err);
    }
    *plan = Plan::new();
    // machine params: file order is machine by machine, then word order
    let mut m = 0;
    while m < b.nm {
        let mut word = 0usize;
        let mut i = 0;
        while i < b.nparam {
            let q = &b.params[i];
            let g = &b.pages[q.page as usize];
            if g.owner as usize == m {
                if g.canvas {
                    return Err(E_FORMAT); // a canvas page binds params, it adds none
                }
                if word >= p.nslots as usize || word >= KNOBS {
                    return Err(E_FORMAT); // more params than the device has knob positions
                }
                plan.file[i] = plan.nfile as u8;
                plan.of_file[plan.nfile] = i as u8;
                plan.nfile += 1;
                plan.pos[i] = p.slots[word] as u8;
                plan.word[i] = word as u8;
                plan.machine[i] = m as u8;
                word += 1;
            }
            i += 1;
        }
        m += 1;
    }
    // PARAM
    let mut k = 0;
    while k < plan.nfile {
        let i = plan.of_file[k] as usize;
        let q = &b.params[i];
        let (gmin, gmax) = match glyph_range(q.glyph) {
            Some(r) => r,
            None => return Err(E_FORMAT),
        };
        if q.min < gmin * 256 || q.max > gmax * 256 {
            return Err(E_FORMAT); // the range does not fit the glyph
        }
        let tpl = p.tpl[q.glyph as usize];
        o.head(KIND_PARAM, LEN_PARAM as usize);
        o.p32(k as u32);
        o.p32(tpl);
        o.p32(p.slot_base + plan.pos[i] as u32);
        o.p32(q.min as u32);
        o.p32(q.max as u32);
        o.p32(q.def as u32);
        o.text(&q.name, 32);
        o.text(&q.short, 16);
        k += 1;
    }
    // MACHINE, then the knob page of each (page devices)
    let mut m = 0;
    while m < b.nm {
        let mc = &b.machines[m];
        let mut knobs = [0u32; KNOBS];
        if !p.pages {
            if p.tune_code != 0 && (p.tune_slot as usize) < KNOBS {
                knobs[p.tune_slot as usize] = p.tune_code;
            }
            if p.level_code != 0 && (p.level_knob as usize) < KNOBS {
                knobs[p.level_knob as usize] = p.level_code;
            }
            let mut i = 0;
            while i < b.nparam {
                if plan.file[i] != u8::MAX && plan.machine[i] as usize == m {
                    knobs[plan.pos[i] as usize] = KNOB_PARAM | plan.file[i] as u32;
                }
                i += 1;
            }
        }
        o.head(KIND_MACHINE, LEN_MACHINE as usize);
        o.p32(machine_key(keys, mc));
        o.p32(p.clone_of);
        o.p32(POS_END);
        o.p32((p.own_dsp && voice[m]) as u32);
        o.p32(p.level);
        let mut j = 0;
        while j < KNOBS {
            o.p32(knobs[j]);
            j += 1;
        }
        o.text(&mc.name, TEXT);
        o.text(&mc.short, TEXT);
        m += 1;
    }
    if p.pages {
        let mut m = 0;
        while m < b.nm {
            let kp = knob_page(b, m)?;
            let mut knobs = [0u32; KNOBS];
            let mut i = 0;
            while i < b.nparam {
                if plan.file[i] != u8::MAX && plan.machine[i] as usize == m {
                    knobs[plan.pos[i] as usize] = KNOB_PARAM | plan.file[i] as u32;
                }
                i += 1;
            }
            let mc = &b.machines[m];
            // the page's own name and short name; a machine without a knob page gets its name in capitals as both
            let mut upper = [0u8; TEXT];
            let (name, tab): (&[u8], &[u8]) = match kp {
                Some(g) => (&b.pages[g].name, &b.pages[g].short),
                None => {
                    let n = len_of(&mc.name);
                    let mut j = 0;
                    while j < n {
                        upper[j] = mc.name[j].to_ascii_uppercase();
                        j += 1;
                    }
                    (&mc.name, &upper)
                }
            };
            o.head(KIND_PAGE, LEN_PAGE as usize);
            o.p32(machine_key(keys, mc));
            let mut j = 0;
            while j < KNOBS {
                o.p32(knobs[j]);
                j += 1;
            }
            o.p32(p.level);
            o.text(tab, TEXT);
            o.text(name, TEXT);
            m += 1;
        }
    } else {
        // the page of a machine-row device still must be unique
        let mut m = 0;
        while m < b.nm {
            knob_page(b, m)?;
            m += 1;
        }
    }
    // ACTION
    let mut i = 0;
    while i < b.nparam {
        let q = &b.params[i];
        if q.click != 0 && plan.file[i] != u8::MAX {
            o.head(KIND_ACTION, LEN_ACTION as usize);
            o.p32(plan.file[i] as u32);
            o.p32(q.click.wrapping_sub(base));
        }
        i += 1;
    }
    // native pages
    let mut m = 0;
    while m < b.nm {
        let mut canvas = [0usize; MAX_PG];
        let mut n = 0;
        let mut i = 0;
        while i < b.np {
            let g = &b.pages[i];
            if g.owner as usize == m && g.canvas {
                canvas[n] = i;
                n += 1;
            }
            i += 1;
        }
        let v1 = n == 1 && b.pages[canvas[0]].flags == 0;
        let mut taken = 0u32;
        let mut j = 0;
        while j < n {
            let g = &b.pages[canvas[j]];
            let index = if g.flags & 1 != 0 {
                0
            } else if g.flags & 2 != 0 {
                1
            } else if v1 {
                1
            } else if taken & 1 == 0 {
                0
            } else {
                1
            };
            taken |= 1 << index;
            if v1 {
                o.head(KIND_NATIVE_PAGE, LEN_NATIVE_PAGE as usize);
                o.p32(1);
            } else {
                o.head(KIND_NATIVE_PAGE_V2, LEN_NATIVE_PAGE_V2 as usize);
                o.p32(2);
            }
            o.p32(machine_key(keys, &b.machines[m]));
            o.p32(g.draw.wrapping_sub(base));
            o.p32(g.event.wrapping_sub(base));
            o.text(&g.name, TEXT);
            if !v1 {
                o.p32(index);
                o.p32(g.flags);
            } else {
                // v1 has the binding table at byte 40 of the body: the title ends there
            }
            let mut bi = 0;
            while bi < 8 {
                if bi < g.nbind as usize {
                    let [kind, value, slot] = g.binds[bi];
                    let v = if kind == 1 {
                        let i = value as usize;
                        if i >= b.nparam || plan.file[i] == u8::MAX {
                            return Err(E_FORMAT);
                        }
                        plan.file[i] as u32
                    } else {
                        value
                    };
                    o.p32(kind);
                    o.p32(v);
                    o.p32(slot);
                } else {
                    o.p32(0);
                    o.p32(0);
                    o.p32(0);
                }
                bi += 1;
            }
            j += 1;
        }
        m += 1;
    }
    // FILTER: the key, the own knobs of the filter's page (knobs 2 and 3) and the names
    let mut f = 0;
    while f < b.nf {
        let fc = &b.filters[f];
        let mut page = None;
        let mut i = 0;
        while i < b.np {
            if b.pages[i].owner as usize == FILTER_OWNER as usize + f {
                page = Some(i);
            }
            i += 1;
        }
        let nknob = match page {
            Some(g) => b.pages[g].nparam as u32,
            None => 0,
        };
        let draw = page.map_or(0, |g| b.pages[g].draw);
        if draw == 0 {
            o.head(KIND_FILTER, LEN_FILTER as usize);
        } else {
            o.head(KIND_FILTER_V2, LEN_FILTER_V2 as usize);
        }
        o.p32(fc.key);
        o.p32(nknob);
        o.text(&fc.name, TEXT);
        o.text(&fc.short, TEXT);
        let mut j = 0;
        while j < crate::filter::OWN_KNOBS as u32 {
            let mut q = None;
            let mut seen = 0;
            let mut i = 0;
            while i < b.nparam {
                if Some(b.params[i].page as usize) == page {
                    if seen == j {
                        q = Some(i);
                    }
                    seen += 1;
                }
                i += 1;
            }
            match q {
                Some(i) => {
                    let q = &b.params[i];
                    let (gmin, gmax) = match glyph_range(q.glyph) {
                        Some(r) => r,
                        None => return Err(E_FORMAT),
                    };
                    if q.min < gmin * 256 || q.max > gmax * 256 {
                        return Err(E_FORMAT); // the range does not fit the glyph
                    }
                    o.p32(q.glyph);
                    o.p32(q.min as u32);
                    o.p32(q.max as u32);
                    o.p32(q.def as u32);
                    o.text(&q.name, 32);
                    o.text(&q.short, 16);
                }
                None => {
                    o.p32(0);
                    o.p32(0);
                    o.p32(0);
                    o.p32(0);
                    o.text(&[], 32);
                    o.text(&[], 16);
                }
            }
            j += 1;
        }
        if draw != 0 {
            o.p32(draw.wrapping_sub(base));
        }
        f += 1;
    }
    Ok(())
}

/// The callback arguments of the ACTION and native page records `lower` wrote, in record order (`apply` reads them through `take_args`).
pub struct ApplyArgs {
    pub click: [u32; 32],
    pub native: [u32; 8],
}

pub fn apply_args(b: &Build, plan: &Plan) -> ApplyArgs {
    let mut a = ApplyArgs {
        click: [0; 32],
        native: [0; 8],
    };
    let mut n = 0;
    let mut i = 0;
    while i < b.nparam {
        if b.params[i].click != 0 && plan.file[i] != u8::MAX {
            a.click[n % 32] = b.params[i].click_arg;
            n += 1;
        }
        i += 1;
    }
    n = 0;
    let mut m = 0;
    while m < b.nm {
        let mut i = 0;
        while i < b.np {
            let g = &b.pages[i];
            if g.owner as usize == m && g.canvas {
                a.native[n % 8] = g.arg;
                n += 1;
            }
            i += 1;
        }
        m += 1;
    }
    a
}

/// The callback arguments of the section the last commit wrote.
pub fn args() -> Option<&'static ApplyArgs> {
    unsafe { (*addr_of!(ARGS)).as_ref() }
}

/// The eight knob ranges and the latch mask of machine `m` for its voice row: knob position j holds `min | max << 16` of the param on it (0: none or out of range);
/// bit w of the mask is the machine's word w.
pub fn voice_declared(b: &Build, plan: &Plan, m: usize) -> ([u32; 8], u32) {
    let mut rng = [0u32; 8];
    let mut latched = 0u32;
    let mut i = 0;
    while i < b.nparam {
        if plan.file[i] != u8::MAX && plan.machine[i] as usize == m {
            let q = &b.params[i];
            let j = plan.pos[i] as usize;
            if q.min >= 0 && q.max <= 0xffff && q.min <= q.max && j < 8 {
                rng[j] = (q.min as u32) | (q.max as u32) << 16;
            }
            if q.latched {
                latched |= 1 << plan.word[i];
            }
        }
        i += 1;
    }
    (rng, latched)
}

// ---- the commit checks -----------------------------------------------------------------------------------------------------------

/// The numbers of free rows a commit needs.
pub struct Free {
    pub machine_rows: u32,
    pub param_rows: u32,
    pub page_rows: u32,
}

/// The rows the module's own declarations use, as `[uses]` counts them: (machine rows, param rows, page rows) of the lowered records.
pub fn counts_of(b: &Build, p: &Profile) -> (u32, u32, u32) {
    let pages = if p.pages { b.nm as u32 } else { 0 };
    (b.nm as u32, b.nparam as u32, pages)
}

/// The checks of 6.3 that need only the collection, the header's keys table and the free rows. Returns OK or the refusal.
pub fn check_commit(b: &Build, p: &Profile, keys: &KeysCopy, free: &Free) -> i32 {
    if b.err != 0 {
        return b.err;
    }
    // every machine index of a builder call is a row of the keys table
    let mut i = 0;
    while i < b.nm {
        if b.machines[i].index as usize >= keys.nm {
            return E_DECL_ID;
        }
        i += 1;
    }
    // the collected rows do not exceed [uses]
    let (m, pa, pg) = counts_of(b, p);
    if m > keys.machine_rows || pa > keys.param_rows || pg > keys.page_rows {
        return E_DECL_USES;
    }
    // a free row for each
    if m > free.machine_rows {
        return E_TYPE_USED;
    }
    if pa > free.param_rows {
        return E_PARAM_FULL;
    }
    if pg > free.page_rows {
        return E_PAGE_FULL;
    }
    OK
}

/// The free reserved machine, parameter and page rows of the registry.
pub unsafe fn free_rows(inf: *const u32) -> Free {
    let s = crate::records::rstate();
    Free {
        machine_rows: capacity::free_rows(&s.mach_owner, iw(inf, w::M_DYN_N)),
        param_rows: capacity::free_rows(&s.param_owner, iw(inf, w::P_DYN_N)),
        page_rows: capacity::free_rows(&s.page_owner, iw(inf, w::PG_DYN_N)),
    }
}

// ---- the commit (Service::module_commit) ---------------------------------------------------------------------------------------

/// Does module record `rec` have a voice row for machine `m` of the collection? Its type is the number the sticky assignment gave the machine's key.
unsafe fn voices<C: CoreApi>(core: &C, rec: usize, b: &Build, keys: &KeysCopy) -> [bool; MAX_M] {
    let mut voice = [false; MAX_M];
    let mut m = 0;
    while m < b.nm {
        let t = assign::resolve(core, machine_key(keys, &b.machines[m]));
        voice[m] = dsp::voice_row(core, rec, t).is_some();
        m += 1;
    }
    voice
}

/// The commit of module record `rec` (`Service::module_commit`): `PREPARE` checks the collection and measures its records, `WRITE` writes them, counts the rows and keeps the callback
/// arguments for `apply`; `DECLARE` (after the registry walker's check) declares the voice rows' ranges. The collection is closed after the `DECLARE` pass.
pub unsafe fn commit<C: CoreApi>(core: &C, rec: usize, c: &ModuleCommitCtx) -> i32 {
    let b = build();
    if !gate().is_open_for(rec) {
        return OK; // no `module_begin` for this record: nothing was collected
    }
    if b.err != 0 {
        return b.err;
    }
    if b.is_empty() {
        end();
        return OK;
    }
    // a provider adds no machine, parameter or page row of its own
    if rec >= dkrt_abi::MAX_MODULES {
        return E_FORMAT;
    }
    let inf = dkrt_svc_params::dynreg(core);
    if inf.is_null() || iw(inf, w::VERSION) < 3 {
        return E_NO_REGISTRY;
    }
    let p = Profile::read(inf);
    // the keys of the filters: the row of each filter's index in the keys table
    let mut fk = [0u32; 8];
    let nfk = core.keys_of(core.boot_index(), KIND_FILTER, &mut fk);
    let mut f = 0;
    while f < b.nf {
        let ix = b.filters[f].index as usize;
        if ix >= nfk {
            return E_DECL_ID;
        }
        b.filters[f].key = fk[ix];
        f += 1;
    }
    let mut plan = Plan::new();
    let voice = voices(core, rec, b, c.keys);
    let sink = &mut *c.sink;
    if c.phase == commit_phase::PREPARE {
        let r = check_commit(b, &p, c.keys, &free_rows(inf));
        if r != OK {
            return r;
        }
        return match lower(b, &p, c.keys, &voice, c.base, sink, &mut plan) {
            Ok(()) => OK,
            Err(e) => e,
        };
    }
    if c.phase == commit_phase::WRITE {
        if let Err(e) = lower(b, &p, c.keys, &voice, c.base, sink, &mut plan) {
            return e;
        }
        *addr_of_mut!(ARGS) = Some(apply_args(b, &plan));
        let (mr, pr, pg) = counts_of(b, &p);
        sink.rows[0] += mr;
        sink.rows[1] += pr;
        sink.rows[2] += pg;
        return OK;
    }
    // DECLARE: the walker's check found nothing wrong. The plan of the lowering gives each machine's knob positions again (a measuring pass writes nothing).
    if let Err(e) = lower(
        b,
        &p,
        c.keys,
        &voice,
        c.base,
        &mut RecordSink::measure(),
        &mut plan,
    ) {
        return e;
    }
    // the ranges and latch mask of each machine's voice row
    let mut m = 0;
    while m < b.nm {
        let t = assign::resolve(core, machine_key(keys_of(c), &b.machines[m]));
        if let Some(row) = dsp::voice_row(core, rec, t) {
            let (rng, latched) = voice_declared(b, &plan, m);
            if latched != 0 {
                core.log(
                    logk::MODULE_LATCH,
                    LOG_INFO,
                    [core.boot_index(), m as u32, latched, 0, 0],
                );
            }
            let r = dsp::set_declared(core, row, rng, latched);
            if r != OK {
                return r;
            }
        }
        m += 1;
    }
    end();
    OK
}

fn keys_of<'a>(c: &'a ModuleCommitCtx) -> &'a KeysCopy {
    c.keys
}
