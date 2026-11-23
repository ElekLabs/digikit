//! The builder calls of the effects (docs/plans/sdk.md 6.2) and their commit (6.3), as the fx service has them: the calls that name an effect, its page and its knobs
//! (`dk_module_ctx_add_fx`, `dk_fx_add_page`, `dk_page_add_fx_param`, `dk_fx_routes`, `dk_fx_mix`) are the service's exports (`dks/src/builder.rs`), and the state they fill is the
//! owner's [`Builder`]: one module is in `init` at a time.
//!
//! `Records::begin` opens the collection for module record `rec`, every call appends to it, and `Records::commit` checks it and lowers it to the effect records of the `register`
//! section in two passes over the loader's `RecordSink` (`PREPARE` checks and measures, `WRITE` writes and declares the route flags and defaults of the effects' DSP rows). The
//! loader gives the block to the registry walker, which calls this service's `check` and `apply`.
//!
//! A call never fails one by one: on overflow, on a bad argument, outside `init` or with a handle that is not ours it returns the placeholder handle (`placeholder`), the
//! first error is kept, and the setters ignore a placeholder. The error is reported at the commit.
use crate::fxpage::{KIND_EFFECT, LEN_EFFECT};
use dkneeds::{FXF_DEFAULT_SEND, FXF_NOMASTER, FXF_SEND};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::MAX_FX;
use dkrt_abi::keys::KeysCopy;
use dkrt_abi::provider::Status;
use dkrt_abi::service::{commit_phase, ModuleCommitCtx, RecordSink};
use dkrt_abi::MAX_MODULES;
use dkrt_sdk::*;
use dkrt_svc_params::builder::{fail, find, len_of, put_text, room, Gate};

const MAX_PA: usize = dkneeds::MAX_RECORDS as usize;
const TEXT: usize = 24;
/// The characters of an effect's short name (the tab label of its page record).
const FX_SHORT: usize = 3;
pub const FX_KNOBS: usize = 7;
pub const ROUTE_MASTER: u32 = dkneeds::page::DK_ROUTE_MASTER;
pub const ROUTE_SEND: u32 = dkneeds::page::DK_ROUTE_SEND;
/// `DK_BIPOLAR` of the display kind of `dk_page_add_fx_param` (`DK_PERCENT` is 1).
const DK_BIPOLAR: u32 = 2;

#[derive(Clone, Copy)]
pub struct CFx {
    /// the effect's index in the module (`DK_FX_ID_<ID>`): the position of its id in the keys table, the ordinal of its effect record
    pub index: u32,
    pub name: [u8; TEXT],
    pub short: [u8; TEXT],
    pub routes_set: bool,
    pub allowed: u32,
    pub def: u32,
    /// the MIX knob default 0 to 127 (`dk_fx_mix`)
    pub mix: u32,
    pub page: i32,
}
#[derive(Clone, Copy)]
pub struct CPage {
    pub owner: u8,
    pub name: [u8; TEXT],
    /// the tab label of the page record (`short_name` of `dk_fx_add_page`)
    pub short: [u8; TEXT],
}
#[derive(Clone, Copy)]
pub struct CParam {
    pub page: u8,
    pub name: [u8; 32],
    pub short: [u8; 16],
    /// display kind and the float bits
    pub kind: u32,
    pub fmin: u32,
    pub fmax: u32,
    pub fdef: u32,
}

const F0: CFx = CFx {
    index: 0,
    name: [0; TEXT],
    short: [0; TEXT],
    routes_set: false,
    allowed: 0,
    def: 0,
    mix: 127,
    page: -1,
};
const G0: CPage = CPage {
    owner: 0,
    name: [0; TEXT],
    short: [0; TEXT],
};
const P0: CParam = CParam {
    page: 0,
    name: [0; 32],
    short: [0; 16],
    kind: 0,
    fmin: 0,
    fmax: 0,
    fdef: 0,
};

/// What `init` declared of the effects.
pub struct Build {
    pub err: i32,
    pub fxs: [CFx; MAX_FX],
    pub nf: usize,
    pub pages: [CPage; MAX_FX],
    pub np: usize,
    pub params: [CParam; MAX_PA],
    pub nparam: usize,
    records: usize,
}

impl Build {
    pub const fn new() -> Build {
        Build {
            err: 0,
            fxs: [F0; MAX_FX],
            nf: 0,
            pages: [G0; MAX_FX],
            np: 0,
            params: [P0; MAX_PA],
            nparam: 0,
            records: 0,
        }
    }

    /// Nothing was declared.
    pub fn is_empty(&self) -> bool {
        self.nf + self.np + self.nparam == 0
    }

    fn fail(&mut self, e: i32) {
        fail(&mut self.err, e);
    }

    /// One more record of the bounded collection; false (error kept) when full.
    fn room(&mut self) -> bool {
        room(&mut self.err, &mut self.records, MAX_PA)
    }

    pub fn add_fx(
        &mut self,
        gate: &Gate,
        index: u32,
        name: *const u8,
        short: *const u8,
    ) -> *mut u8 {
        if self.nf >= MAX_FX || !self.room() {
            self.fail(E_DECL_FULL);
            return gate.placeholder();
        }
        let i = self.nf;
        let mut f = F0;
        f.index = index;
        let mut e = 0;
        unsafe {
            put_text(&mut f.name, name, &mut e);
            put_text(&mut f.short, short, &mut e);
        }
        if len_of(&f.short) > FX_SHORT {
            e = E_FORMAT; // the effect record's tab label holds 3 characters: refused, not cut
        }
        self.fail(e);
        self.fxs[i] = f;
        self.nf += 1;
        &mut self.fxs[i] as *mut CFx as *mut u8
    }

    pub fn fx_add_page(
        &mut self,
        gate: &Gate,
        f: *mut u8,
        name: *const u8,
        short: *const u8,
    ) -> *mut u8 {
        let owner = match find(&self.fxs, f) {
            Some(i) => i,
            None => return gate.placeholder(),
        };
        if self.np >= MAX_FX || !self.room() {
            self.fail(E_DECL_FULL);
            return gate.placeholder();
        }
        let i = self.np;
        let mut g = G0;
        g.owner = owner as u8;
        let mut e = 0;
        unsafe {
            put_text(&mut g.name, name, &mut e);
            put_text(&mut g.short, short, &mut e);
        }
        if len_of(&g.short) > FX_SHORT {
            e = E_FORMAT; // the effect record's tab label holds 3 characters: refused, not cut
        }
        self.fail(e);
        if self.fxs[owner].page >= 0 {
            self.fail(E_FORMAT); // an effect has one page
        } else {
            self.fxs[owner].page = i as i32;
        }
        self.pages[i] = g;
        self.np += 1;
        &mut self.pages[i] as *mut CPage as *mut u8
    }

    /// `dk_page_add_fx_param`. A handle of another service (a machine's page) is an effect parameter on a machine page: refused.
    #[allow(clippy::too_many_arguments)]
    pub fn page_add_fx_param(
        &mut self,
        gate: &Gate,
        p: *mut u8,
        name: *const u8,
        short: *const u8,
        kind: u32,
        min: f32,
        max: f32,
        def: f32,
    ) -> *mut u8 {
        let pg = match find(&self.pages, p) {
            Some(i) => i,
            None => {
                if gate.foreign(p) {
                    self.fail(E_FORMAT);
                }
                return gate.placeholder();
            }
        };
        if self.nparam >= MAX_PA || !self.room() {
            self.fail(E_DECL_FULL);
            return gate.placeholder();
        }
        let i = self.nparam;
        let mut q = P0;
        q.page = pg as u8;
        q.kind = kind;
        q.fmin = min.to_bits();
        q.fmax = max.to_bits();
        q.fdef = def.to_bits();
        let mut e = 0;
        unsafe {
            put_text(&mut q.name, name, &mut e);
            put_text(&mut q.short, short, &mut e);
        }
        self.fail(e);
        self.params[i] = q;
        self.nparam += 1;
        &mut self.params[i] as *mut CParam as *mut u8
    }

    pub fn fx_routes(&mut self, f: *mut u8, allowed: u32, def: u32) {
        if let Some(i) = find(&self.fxs, f) {
            self.fxs[i].routes_set = true;
            self.fxs[i].allowed = allowed;
            self.fxs[i].def = def;
        }
    }

    pub fn fx_mix(&mut self, f: *mut u8, def: u32) {
        if let Some(i) = find(&self.fxs, f) {
            self.fxs[i].mix = def.min(127);
        }
    }
}

// ---- the open collection and the placeholder ------------------------------------------------------------------------------------

/// The collection of one module's `init` and the gate that says whose `init` is open: the part of the owner `Fx` that the builder calls reach (`Fx::builder`). The calls
/// that name a handle are refused (the placeholder) unless the gate is open for the module whose code runs.
pub struct Builder {
    build: Build,
    gate: Gate,
}

impl Builder {
    pub const fn new() -> Builder {
        Builder {
            build: Build::new(),
            gate: Gate::new(),
        }
    }

    /// The placeholder handle of a refused call.
    pub fn placeholder(&self) -> *mut u8 {
        self.gate.placeholder()
    }

    /// The collection (host tests read it).
    pub fn build(&self) -> &Build {
        &self.build
    }

    /// The collection, for a host test that fills it by hand.
    pub fn build_mut(&mut self) -> &mut Build {
        &mut self.build
    }

    /// `Records::begin`: forget what an earlier module collected and open the collection for module record `rec`.
    pub fn begin(&mut self, rec: usize) {
        self.build = Build::new();
        self.gate.begin(rec);
    }

    /// Close the collection: later builder calls return the placeholder.
    pub fn end(&mut self) {
        self.gate.end();
    }

    // the builder calls (exported by the service file: `dks/src/builder.rs`); a call that names a handle comes from the module whose `init` is open

    /// # Safety
    /// `ctx` is any pointer (the core only compares it); `name` and `short` are C strings or null.
    pub unsafe fn add_fx<C: CoreApi>(
        &mut self,
        core: &C,
        ctx: *mut u8,
        index: u32,
        name: *const u8,
        short: *const u8,
    ) -> *mut u8 {
        if self.gate.open_for(core, ctx) {
            self.build.add_fx(&self.gate, index, name, short)
        } else {
            self.placeholder()
        }
    }

    /// # Safety
    /// `f` is any pointer; `name` and `short` are C strings or null.
    pub unsafe fn fx_add_page<C: CoreApi>(
        &mut self,
        core: &C,
        f: *mut u8,
        name: *const u8,
        short: *const u8,
    ) -> *mut u8 {
        if self.gate.open_any(core) {
            self.build.fx_add_page(&self.gate, f, name, short)
        } else {
            self.placeholder()
        }
    }

    /// # Safety
    /// `p` is any pointer; `name` and `short` are C strings or null.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn page_add_fx_param<C: CoreApi>(
        &mut self,
        core: &C,
        p: *mut u8,
        name: *const u8,
        short: *const u8,
        kind: u32,
        min: f32,
        max: f32,
        def: f32,
    ) -> *mut u8 {
        if self.gate.open_any(core) {
            self.build
                .page_add_fx_param(&self.gate, p, name, short, kind, min, max, def)
        } else {
            self.placeholder()
        }
    }

    pub fn fx_routes<C: CoreApi>(&mut self, core: &C, f: *mut u8, allowed: u32, def: u32) {
        if self.gate.open_any(core) {
            self.build.fx_routes(f, allowed, def);
        }
    }

    pub fn fx_mix<C: CoreApi>(&mut self, core: &C, f: *mut u8, def: u32) {
        if self.gate.open_any(core) {
            self.build.fx_mix(f, def);
        }
    }
}

// ---- lowering to records -----------------------------------------------------------------------------------------------------

/// The id of an effect of the collection: the keys table row at its index.
pub fn fx_id(keys: &KeysCopy, f: &CFx) -> u32 {
    let i = f.index as usize;
    if i < keys.nf {
        keys.fk[i]
    } else {
        0
    }
}

/// Lower the collection into `o`: the EFFECT record of each effect with a page (its tab label is the first three characters of its short name). `o` without a buffer only
/// measures. The errors are refusals of the commit.
pub fn lower(b: &Build, keys: &KeysCopy, o: &mut RecordSink) -> Result<(), Status> {
    if b.err != 0 {
        return Err(b.err);
    }
    let mut f = 0;
    while f < b.nf {
        let fx = &b.fxs[f];
        if fx.page >= 0 {
            let g = &b.pages[fx.page as usize];
            let mut knobs = [0usize; FX_KNOBS];
            let mut n = 0;
            let mut i = 0;
            while i < b.nparam {
                if b.params[i].page as i32 == fx.page {
                    if n >= FX_KNOBS {
                        return Err(E_FORMAT);
                    }
                    knobs[n] = i;
                    n += 1;
                }
                i += 1;
            }
            o.head(KIND_EFFECT, LEN_EFFECT as usize);
            o.p32(fx.index); // the effect record of the DSP section: the keys table order
            o.p32(fx.mix); // MIX default
            o.p32(n as u32);
            o.p32(fx_id(keys, fx));
            let mut tab = [0u8; 8];
            let label = if len_of(&g.short) > 0 {
                &g.short
            } else {
                &fx.short
            };
            let mut j = 0;
            while j < FX_SHORT && j < len_of(label) {
                tab[j] = label[j];
                j += 1;
            }
            o.text(&tab, 8);
            o.text(&g.name, TEXT);
            let mut j = 0;
            while j < FX_KNOBS {
                if j < n {
                    let q = &b.params[knobs[j]];
                    if len_of(&q.name) >= TEXT || len_of(&q.short) > 6 {
                        return Err(E_FORMAT);
                    }
                    o.p32(j as u32); // position: the knob's place in the list
                    o.p32(if q.kind == DK_BIPOLAR { 1 } else { 0 });
                    o.p32(q.fmin);
                    o.p32(q.fmax);
                    o.p32(q.fdef);
                    o.text(&q.name, TEXT);
                    o.text(&q.short, 8);
                } else {
                    o.text(&[], 52);
                }
                j += 1;
            }
        }
        f += 1;
    }
    Ok(())
}

/// The route flags of the DSP effect record (`FXF_*`) for `dk_fx_routes(allowed, def)`.
pub fn fx_flags(allowed: u32, def: u32) -> u32 {
    let mut f = 0;
    if allowed & ROUTE_SEND != 0 {
        f |= FXF_SEND;
    }
    if allowed & ROUTE_MASTER == 0 {
        f |= FXF_NOMASTER;
    }
    if def == ROUTE_SEND {
        f |= FXF_DEFAULT_SEND;
    }
    f
}

/// The flags of effect `fx` of the collection: `dk_fx_routes` when it was called, else the master only (allowed and default MASTER).
pub fn fx_flags_of_decl(fx: &CFx) -> u32 {
    if fx.routes_set {
        fx_flags(fx.allowed, fx.def)
    } else {
        fx_flags(ROUTE_MASTER, ROUTE_MASTER)
    }
}

/// The words p0 .. of an effect's row: the defaults of the knobs of its page (float bits), 0 after them.
pub fn fx_defaults(b: &Build, f: usize) -> [u32; 8] {
    let mut out = [0u32; 8];
    let fx = &b.fxs[f];
    if fx.page < 0 {
        return out;
    }
    let mut n = 0;
    let mut i = 0;
    while i < b.nparam {
        if b.params[i].page as i32 == fx.page && n < 8 {
            out[n] = b.params[i].fdef;
            n += 1;
        }
        i += 1;
    }
    out
}

// ---- the commit checks ---------------------------------------------------------------------------------------------------------

/// The checks of 6.3 that need only the collection and the header's keys table. Returns OK or the refusal. An effect id that is twice in the file or in another loaded module
/// is the planner's refusal (`E_PLAN_KEY`, at the boot and at a live load or reload), not this check's.
pub fn check_commit(b: &Build, keys: &KeysCopy) -> i32 {
    if b.err != 0 {
        return b.err;
    }
    // every effect index of a builder call is a row of the keys table
    let mut i = 0;
    while i < b.nf {
        if b.fxs[i].index as usize >= keys.nf {
            return E_DECL_ID;
        }
        i += 1;
    }
    OK
}

// ---- the commit (Records::commit) ---------------------------------------------------------------------------------------------

impl Builder {
    /// The commit of module record `rec` (`Records::commit`): `PREPARE` checks the collection and measures its records, `WRITE` writes them; `DECLARE` (after the registry walker's
    /// check) declares the route flags and defaults of the effects' DSP rows. The collection is closed after the `DECLARE` pass.
    ///
    /// # Safety
    /// `c.sink` is the loader's sink.
    pub unsafe fn commit<C: CoreApi>(
        &mut self,
        core: &C,
        rec: usize,
        c: &ModuleCommitCtx,
    ) -> Result<(), Status> {
        let b = &self.build;
        if !self.gate.is_open_for(rec) {
            return Ok(()); // no `begin` for this record: nothing was collected
        }
        if b.err != 0 {
            return Err(b.err);
        }
        // a provider adds no effect of its own
        if rec >= MAX_MODULES {
            return if b.is_empty() { Ok(()) } else { Err(E_FORMAT) };
        }
        let sink = &mut *c.sink;
        if c.phase == commit_phase::PREPARE {
            if b.is_empty() {
                return Ok(());
            }
            let r = check_commit(b, c.keys);
            if r != OK {
                return Err(r);
            }
            return lower(b, c.keys, sink);
        }
        if c.phase == commit_phase::WRITE {
            if !b.is_empty() {
                lower(b, c.keys, sink)?;
            }
            return Ok(());
        }
        // DECLARE: the walker's check found nothing wrong
        if !b.is_empty() {
            // the routes and defaults of each effect's row; without `dk_fx_routes` the effect runs on the master only (the rule of the old `routes` field)
            let mut f = 0;
            while f < b.nf {
                let fx = &b.fxs[f];
                let par = if fx.page >= 0 {
                    Some(fx_defaults(b, f))
                } else {
                    None
                };
                if !crate::dsp::fx_set_declared(
                    core,
                    rec,
                    fx.index,
                    Some(fx_flags_of_decl(fx)),
                    par,
                ) {
                    return Err(E_FORMAT); // no such effect in the module's DSP section
                }
                f += 1;
            }
        }
        self.end();
        Ok(())
    }
}
