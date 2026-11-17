//! The `.dkm` loader (docs/design/digikit-dkm.md). Every check runs before anything is written to the arena.

use crate::dsp;
use crate::entry::Ctx;
use discover::Found;
use crate::handlers::{services, ServiceTable};
use crate::plan::{self, Env, Plan, TABLE_MAX};
use dkneeds::keys::Keys;
use install::{Backend, DspInstaller, Installer};
use role::{PlaceArgs, RoleSteps};
use exports::Exports;
use crate::platform::RealPlatform;
use crate::selection::BOOT_IDX;
use crate::{cstr_eq, cstr_len, rd32, st, wr32, Module, MAX_MODULES};
use core::ptr::{addr_of, addr_of_mut, read_volatile};
use dkrt_abi::link::Owner;
use dkrt_abi::platform::Platform;
use dkrt_sdk::*;

/// `init`, `fini`, `menu` and `tick` are image offsets in the header (NONE for a missing one): the fields of the module's `dk_module` constant
/// (`dk_module_def`), called with a `dk_module_ctx *` (`entry.rs`).
const NONE: u32 = dkneeds::NONE;

// header field offsets (big endian): `[format]` of sdk/sdk.toml; the parse in `image.rs` reads the rest
const H_VERSION: usize = dkneeds::H_VERSION as usize;
const H_NAME: usize = dkneeds::H_NAME as usize;
const H_REG_OFF: usize = dkneeds::H_REG_OFF as usize;
const H_REG_SIZE: usize = dkneeds::H_REG_SIZE as usize;
const H_DSP_OFF: usize = dkneeds::H_DSP_OFF as usize;
const H_DSP_SIZE: usize = dkneeds::H_DSP_SIZE as usize;

pub(crate) mod discover;
pub(crate) mod exports;
mod image;
pub(crate) mod install;
mod records;
pub(crate) mod role;
mod seams;
#[cfg(test)]
mod tests;
use image::in_file;
pub use seams::{Block, Images, RealImages, RealRegistry, Registry, Staged};

/// The index the log shows for a provider file (`MOD_LOAD` and the events of its load): not a selection index, and not `u32::MAX` (no load running).
pub const PROVIDER_IDX: u32 = 0xffff_fffe;

/// What a provider record keeps after its load started: the copy of its header table (BIND, EXPORTS and SLOTS; the staged file is gone once `init` reads
/// from the store) and the size of its image.
#[derive(Clone, Copy)]
pub(crate) struct ProvRec {
    pub block: Option<Block>,
    pub len: u32,
    pub image_size: u32,
    /// Bytes of the DSP image of the file (0: it has no DSP section): the bound of the entries of its DSP slot fills.
    pub dsp_image: u32,
}

impl ProvRec {
    pub const EMPTY: ProvRec = ProvRec { block: None, len: 0, image_size: 0, dsp_image: 0 };
}

/// Which loads the loader accepts. The module set is fixed per session: only the boot hook loads (hot reload is archived, docs/plans/digikit-hot-reload.md).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// The state after reset and after the boot's loop: `load` is `E_BOOT_ONLY`.
    Closed = 0,
    /// While the boot hook loads the selection: the CF part of a module with DSP code is published at once (the machine view is built soon after the
    /// hook, and the frames that carry DSP code do not flow yet); the DSP part follows over the UI-loop passes.
    Boot = 1,
}

/// The module lifecycle: stage, check, place, init, commit (and DSP) with an undo for each step, and the boot's planning tables. One zero-initialised static
/// (`LOADER`); its methods take `&mut self`.
///
/// What module code can reach while `init` or `fini` runs is not in here: the module records (`DKRT_STATE.modules`), `DKRT_PATHS`, `DKRT_MENUS`,
/// `DKRT_MODULE_RUNTIME`, `DKRT_CTXS`, `DKRT_TICKS`, `BOOT_IDX`, `CALLED_NEED`, `collect::ACTIVE`, `DKRT_BOOT` and the service rows. The loader is their
/// only writer.
pub struct Loader<P: Platform, M: Images, R: Registry> {
    p: P,
    m: M,
    r: R,
    phase: Phase,
    /// The role word of each record's file (0 module, 1 service): which `RoleSteps` its undo uses.
    roles: [u8; records::REC_MAX],
    /// The name of the missing module of the last load that failed with E_IMPORT (`import_needs`), NUL padded; empty when none. The boot clears it before
    /// each load and takes it after.
    missing: [u8; NEEDS_LEN],
    /// The first item that `init`'s declarations and the header table do not both hold, of the last load that failed with E_DECL_KIND: (tag of the item, its first
    /// word), and 0x100 added to the tag when only the header has it. `(0, 0)`: none (`report_provider` logs it).
    decl_diff: (u32, u32),
    /// The header table of the module at each selection index, kept while the boot reads each file once (`capture`); `table_len` 0: none (the file
    /// is missing, torn, of the old format or has a larger table).
    tables: [[u8; TABLE_MAX]; SEL_MAX],
    table_len: [usize; SEL_MAX],
    /// The DSP load of the accepted modules of the last plan: (load, largest per-voice cost), which `dkrt_capacity` reports as used and largest of
    /// `CAPK_DSP_COST`; the module set is fixed per session, so the plan is the load.
    planned_dsp: (u32, u32),
    /// The providers the boot's search found: their paths and tables. Provider `j` is record `MAX_MODULES + j`.
    found: Found,
    /// Whether provider `j` of the plan loaded (settle reads it).
    loaded: [bool; PROVIDER_MAX],
    /// The table copy of each provider record (`role.rs`).
    provs: [ProvRec; PROVIDER_MAX],
    /// The functions the loaded providers offer by name; an import that the runtime's table lacks resolves here.
    exports: Exports,
    /// The hook sites and slot words of the loaded providers in the image (the ColdFire backend). Not armed in safe mode.
    installer: Installer,
    /// The slots of the providers' DSP code in core/dsp-link's slot block (the DSP backend).
    dsp_install: DspInstaller,
    /// The selection index of each module record (`u8::MAX`: none): the late teardown of a service names the dependents' states by it.
    sel_of_rec: [u8; MAX_MODULES],
}

/// The loader of the device.
type Real = Loader<RealPlatform, RealImages, RealRegistry>;

static mut LOADER: Real = Loader::new(RealPlatform, RealImages, RealRegistry);

fn loader() -> &'static mut Real {
    unsafe { &mut *addr_of_mut!(LOADER) }
}

/// The address a loaded service file published for the export `hash`, if any (`handlers::exports::find_export`).
pub fn find_export(hash: u32) -> Option<u32> {
    loader().exports.resolve(hash)
}

/// The boot's load loop over the selection in plan order (`Loader::boot`).
pub unsafe fn boot(r: &dkrt_sdk::sel::Record, planned: &Plan) {
    loader().boot(r, planned)
}

/// Validate and load the `.dkm` at `path`. See `dkrt_load`.
pub unsafe fn load(path: *const u8) -> i32 {
    loader().load(path)
}

/// The boot's provider stage (`Loader::load_providers`): the providers of the plan load, in dependency order, before any module.
pub unsafe fn load_providers(planned: &Plan) {
    loader().load_providers(planned)
}

/// The settle step (`Loader::settle`): a module whose provider did not load is refused.
pub unsafe fn settle(planned: &mut Plan, count: usize) {
    loader().settle(planned, count)
}

/// Safe mode: the installer writes nothing.
pub unsafe fn set_safe() {
    loader().installer.set_armed(false)
}

/// The DSP part of a module published at boot failed: take it down again.
pub unsafe fn undo_early(rec: usize) -> i32 {
    loader().undo_early(rec)
}

/// Take module record `rec` down (`Loader::unload_rec`).
pub unsafe fn unload_rec(rec: usize) -> i32 {
    loader().unload_rec(rec)
}

/// One UI-loop pass of the DSP loader. `dsp` only advances its state and reports what it found; the module teardown, the machine
/// listing and the boot result belong to the loader, so `dsp` depends on neither the loader nor the boot.
pub unsafe fn dsp_step() {
    dsp_step_with(services())
}

/// `dsp_step` over a given list of services.
pub(crate) unsafe fn dsp_step_with(l: ServiceTable) {
    loader().dsp_step_with(l)
}

/// Keep the header table of the `.dkm` `file` (the whole file in the staging buffer) for selection index `k`.
pub unsafe fn capture(k: usize, file: &[u8]) {
    loader().capture(k, file)
}

/// Forget the tables (the start of a boot's planning).
pub unsafe fn reset_plan() {
    loader().reset_plan()
}

/// Plan the selection `count` modules long from the tables `capture` kept, with the providers the search found for its hard needs (`Loader::plan_with_caps`).
pub unsafe fn plan_selection(count: usize) -> Plan {
    let mut caps = [DkrtCap { kind: 0, limit: 0, used: 0, largest: 0 }; CAP_MAX];
    let k = crate::dkrt_capacity(caps.as_mut_ptr(), CAP_MAX as u32) as usize;
    // the report counts the selection's paths as used from the start of the boot (`boot::selected`); the plan counts them one by one as it takes the modules
    let mut c = 0;
    while c < k {
        if caps[c].kind == dkrt_sdk::proto::CAPK_SELECTION as u32 {
            caps[c].used = 0;
            caps[c].largest = caps[c].limit;
        }
        // the plan starts with nothing loaded (the report counts the plan's own result from its end)
        if caps[c].kind == dkrt_sdk::proto::CAPK_DSP_COST as u32 {
            caps[c].used = 0;
            caps[c].largest = 0;
        }
        c += 1;
    }
    loader().plan_with_caps(count, &caps[..k])
}

/// The DSP load of the last plan: (load, largest per-voice cost). A read of the field that takes no reference: module code can reach it (`dkrt_capacity`) while
/// a load runs.
pub unsafe fn planned_dsp() -> (u32, u32) {
    *addr_of!(LOADER.planned_dsp)
}

impl<P: Platform, M: Images, R: Registry> Loader<P, M, R> {
    pub const fn new(p: P, m: M, r: R) -> Self {
        Loader {
            p,
            m,
            r,
            phase: Phase::Closed,
            roles: [0; records::REC_MAX],
            missing: [0; NEEDS_LEN],
            decl_diff: (0, 0),
            tables: [[0; TABLE_MAX]; SEL_MAX],
            table_len: [0; SEL_MAX],
            planned_dsp: (0, 0),
            found: Found::new(),
            loaded: [false; PROVIDER_MAX],
            provs: [ProvRec::EMPTY; PROVIDER_MAX],
            exports: Exports::new(),
            installer: Installer::new(),
            dsp_install: DspInstaller::new(),
            sel_of_rec: [u8::MAX; MAX_MODULES],
        }
    }

    /// The name of the missing module of the last load that failed with `E_IMPORT`, NUL padded; empty when none.
    pub fn missing(&self) -> [u8; NEEDS_LEN] {
        self.missing
    }

    /// The boot's load loop: the selection in plan order. A module the plan refuses is not loaded; each result goes to `boot::report_module`.
    pub unsafe fn boot(&mut self, r: &dkrt_sdk::sel::Record, planned: &Plan) {
        self.phase = Phase::Boot;
        let mut pos = 0;
        while pos < r.count as usize {
            let k = planned.order[pos] as usize;
            crate::selection::BOOT_IDX = k as u32;
            self.missing = [0; NEEDS_LEN];
            let res = match planned.verdicts[k] {
                dkrt_abi::plan::Verdict::Accepted => {
                    crate::boot::states().init(k);
                    let res = self.load(r.paths[k].as_ptr());
                    crate::boot::states().finished(k, res);
                    res
                }
                dkrt_abi::plan::Verdict::Refused(why) => why.code(),
            };
            crate::boot::report_module(k, r.paths[k].as_ptr(), res, &self.missing);
            pos += 1;
        }
        self.phase = Phase::Closed;
    }

    /// Validate and load the `.dkm` at `path`. The tables are checked after every attempt, and none starts once a check has failed (`invariants::check`).
    pub unsafe fn load(&mut self, path: *const u8) -> i32 {
        self.load_file(path, None)
    }

    /// Load the service file at `path` into provider record `j`.
    unsafe fn load_provider(&mut self, path: *const u8, j: usize) -> i32 {
        self.load_file(path, Some(MAX_MODULES + j))
    }

    unsafe fn load_file(&mut self, path: *const u8, fixed: Option<usize>) -> i32 {
        if crate::invariants::bad() {
            return E_TABLE;
        }
        let r = self.load_inner(path, fixed);
        #[cfg(not(test))]
        crate::invariants::check();
        r
    }

    /// `fixed`: the provider record of a service file; `None`: a module takes the first free module record. A service file loads only into a provider
    /// record, and a module file never does.
    unsafe fn load_inner(&mut self, path: *const u8, fixed: Option<usize>) -> i32 {
        dsp::init_once();
        // the module set is fixed per session: only the boot hook loads (hot reload is archived, docs/plans/digikit-hot-reload.md)
        if self.phase != Phase::Boot {
            return E_BOOT_ONLY;
        }
        let staged = match self.m.stage(path) {
            Ok(s) => s,
            Err(e) => return e,
        };
        let f = staged.ptr;
        let size = staged.size;

        // ---- checks (nothing is written outside the staging buffer) ----
        let head = match image::head(f, size) {
            Ok(h) => h,
            Err(e) => return e,
        };
        let image::Head { needs, image_off, image_size, bss, reloc_off, reloc_n, import_off, import_n, init, fini, menu, tick } = head;
        // the steps of the file's role
        let role = rd32(f.add(dkneeds::H_ROLE as usize));
        if (role == 1) != fixed.is_some() {
            return E_FORMAT;
        }
        let steps = RoleSteps::<P, M, R>::of(role);
        let r = (steps.check)(self, f, &head);
        if r != OK {
            return r;
        }
        let name = f.add(H_NAME);
        let total = image_size + bss;
        // imports: every name must be in the runtime's table
        let mut import_addrs = [0u32; dkneeds::MAX_CF_IMPORTS as usize];
        let mut i = 0;
        while i < import_n as usize {
            let p = f.add(import_off as usize + i * dkneeds::CF_IMPORT_LEN as usize);
            if cstr_len(p, dkneeds::CF_IMPORT_LEN as usize) == dkneeds::CF_IMPORT_LEN as usize {
                return E_FORMAT;
            }
            let mut a = self.m.import_addr(p);
            if a == 0 && role == 1 {
                // a service file also links the service-only imports (`SVC_IMPORTS`)
                a = self.m.svc_import_addr(p);
            }
            if a == 0 {
                // the runtime's table has no such name: a loaded provider may export it
                let n = cstr_len(p, dkneeds::CF_IMPORT_LEN as usize);
                a = self.exports.resolve(dkneeds::keys::name_hash_bytes(core::slice::from_raw_parts(p, n))).unwrap_or(0);
            }
            if a == 0 {
                // the first import that belongs to a module that is not in the image: the boot keeps its name (`MISSING`)
                if let (Some(n), true) = (self.m.import_needs(p), self.missing[0] == 0) {
                    let b = n.as_bytes();
                    let mut k = 0;
                    while k < b.len() && k < NEEDS_LEN {
                        self.missing[k] = b[k];
                        k += 1;
                    }
                }
                return E_IMPORT;
            }
            import_addrs[i] = a;
            i += 1;
        }
        // relocations: kind, bounds, import index
        if let Err(e) = image::sections(f, &head) {
            return e;
        }
        let s = st();
        // `register` records (machines, parameters): validated now, written after the copy
        let (reg_off, reg_size) = (rd32(f.add(H_REG_OFF)), rd32(f.add(H_REG_SIZE)));
        if !in_file(reg_off, reg_size, size) {
            return E_FORMAT;
        }
        let reg = f.add(reg_off as usize);
        // the table is the keys table; the records come from the builder calls of `init` (complete)
        let keys = match crate::collect::KeysCopy::from_bytes(core::slice::from_raw_parts(reg, reg_size as usize)) {
            Ok(k) => k,
            Err(e) => return e,
        };
        // the DSP section, if any
        let (dsp_off, dsp_size) = (rd32(f.add(H_DSP_OFF)), rd32(f.add(H_DSP_SIZE)));
        let mut job = dsp::Job::empty();
        if dsp_size != 0 {
            let r = dsp::check(f, dsp_off, dsp_size, size, &mut job);
            if r != OK {
                return r;
            }
            let (v, stride) = (job.voices, job.vstride as usize);
            let r = crate::collect::check_dsp_keys(&keys, job.nvoice as usize, |i| rd32(v.add(stride * i)), job.nfx as usize);
            if r != OK {
                return r;
            }
        }
        // already loaded; a free record
        let rec = match fixed {
            Some(rec) => {
                if records::module(rec).used != 0 {
                    return E_LOADED;
                }
                rec
            }
            None => match capacity::pick_record(MAX_MODULES, |i| s.modules[i].used != 0, |i| cstr_eq(s.modules[i].name.as_ptr(), name)) {
                Ok(r) => r,
                Err(e) => return e,
            },
        };

        // ---- allocate, copy, relocate, zero bss ----
        // The DSP range, the relocated DSP image and the voice rows come first: nothing else is allocated yet if they fail.
        if dsp_size != 0 {
            let r = dsp::reserve(rec, &job);
            if r != OK {
                return r;
            }
        }
        let blk = match self.m.alloc(total) {
            Some(b) => b,
            None => {
                if dsp_size != 0 {
                    dsp::cancel(rec);
                }
                return E_NOMEM;
            }
        };
        let (mem, base) = (blk.ptr, blk.addr);
        i = 0;
        while i < image_size as usize {
            *mem.add(i) = *f.add(image_off as usize + i);
            i += 1;
        }
        i = 0;
        while i < bss as usize {
            *mem.add(image_size as usize + i) = 0;
            i += 1;
        }
        i = 0;
        while i < reloc_n as usize {
            let e = f.add(reloc_off as usize + i * dkneeds::CF_RELOC_LEN as usize);
            let off = rd32(e.add(dkneeds::CF_RELOC_OFFSET as usize)) as usize;
            let kind = rd32(e.add(dkneeds::CF_RELOC_KIND as usize));
            let add = if kind >> 24 == 0 {
                base
            } else {
                import_addrs[(kind & 0xff_ffff) as usize]
            };
            let p = mem.add(off);
            wr32(p, rd32(p).wrapping_add(add));
            i += 1;
        }
        // ---- caches: write the lines back and drop them from both caches ----
        self.p.flush(base, total);

        // ---- record, init, publish ----
        self.roles[rec] = (role == 1) as u8;
        if rec < MAX_MODULES {
            let k = read_volatile(addr_of!(BOOT_IDX));
            self.sel_of_rec[rec] = if (k as usize) < SEL_MAX { k as u8 } else { u8::MAX };
        }
        let m: &mut Module = records::module(rec);
        m.used = 1;
        i = 0;
        while i < dkneeds::NAME_LEN as usize {
            m.name[i] = *name.add(i);
            i += 1;
        }
        let dst = records::path(rec);
        let plen = cstr_len(path, 63);
        i = 0;
        while i < 64 {
            dst[i] = if i < plen { *path.add(i) } else { 0 };
            i += 1;
        }
        m.version = rd32(f.add(H_VERSION));
        records::set_runtime(rec, needs & !crate::FLAG_TRACE);
        m.base = base;
        m.size = total;
        m.fini = if fini == NONE { 0 } else { base + fini };
        crate::entry::bind(rec);
        records::set_menu(rec, if menu == NONE { 0 } else { base + menu });
        let place = PlaceArgs { file: f, keys_off: reg_off, keys_len: reg_size, image_size, dsp_image: if dsp_size != 0 { job.image_size } else { 0 } };
        let initaddr = if init == NONE { 0 } else { base + init };
        let tickaddr = if tick == NONE { 0 } else { base + tick };
        if dsp_size != 0 {
            // the CF part goes live now (rows in place before the machine view is built); the DSP code and the voice rows follow
            // when the frames flow
            m.used = 1;
            let r = self.complete(rec, &keys, initaddr, tickaddr, image_size, steps, place);
            if r != OK {
                if dsp::retained() { return E_BUSY; }
                dsp::cancel(rec);
                if self.cannot_retire(rec) { return E_BUSY; }
                self.m.free(base);
                m.used = 0;
                crate::entry::unbind(rec);
                return r;
            }
            dsp::launch(dsp::Pending {
                rec: rec as u32,
                reg: 0,
                reg_size: 0,
                init: 0,
                early: 1,
                boot_idx: read_volatile(addr_of!(BOOT_IDX)),
                send_base: 0,
                send_len: 0,
                send_buf: 0,
            });
            return LOADING;
        }
        let r = self.complete(rec, &keys, initaddr, tickaddr, image_size, steps, place);
        if r != OK {
            if self.cannot_retire(rec) { return E_BUSY; }
            self.m.free(base);
            m.used = 0;
            crate::entry::unbind(rec);
        }
        r
    }


    /// The end of a load, shared by the immediate and the DSP path (both run in the boot hook; docs/plans/sdk.md 6.3): `init` runs first and its builder calls are collected; then the commit checks them, fills the
    /// voice rows' ranges, writes the registry rows and keeps the record block. Anything that fails undoes what `init` did and writes no row.
    #[allow(clippy::too_many_arguments)]
    unsafe fn complete(&mut self, rec: usize, keys: &crate::collect::KeysCopy, init: u32, tick: u32, image_size: u32, steps: RoleSteps<P, M, R>, place: PlaceArgs) -> i32 {
        let s = st();
        let module = rec < MAX_MODULES;
        // the role's part of the file that `init` could overwrite in the staging buffer
        let r = (steps.place)(self, rec, &place);
        if r != OK {
            return self.fail(rec, r);
        }
        if module {
            crate::files::init_begin(rec);
        }
        let blk = match self.m.alloc(core::mem::size_of::<crate::collect::Collect>() as u32) {
            Some(b) => b,
            None => return self.fail(rec, E_NOMEM),
        };
        let block = blk.ptr;
        crate::collect::begin(block as *mut crate::collect::Collect);
        self.r.module_begin(rec);
        if init != 0 {
            let ctx = crate::entry::ctx_addr(rec) as usize as *const Ctx;
            CALLED_NEED = [0; NEEDS_LEN];
            let r = self.m.enter_init(rec, init, ctx);
            if r != 0 {
                crate::collect::end();
                self.m.free(blk.addr);
                let missing = r == E_MISSING && CALLED_NEED[0] != 0;
                if missing && self.missing[0] == 0 {
                    self.missing = CALLED_NEED;
                }
                let e = self.fail(rec, if missing { E_IMPORT } else { E_INIT });
                return if e == E_BUSY { e } else if missing { E_IMPORT } else { E_INIT };
            }
        }
        crate::collect::end();
        let r = self.commit(rec, &*(block as *const crate::collect::Collect), keys, image_size);
        self.m.free(blk.addr);
        if r != OK {
            return self.fail(rec, r);
        }
        let r = (steps.activate)(self, rec);
        if r != OK {
            return self.fail(rec, r);
        }
        let m: &mut Module = records::module(rec);
        m.used = 1;
        // a provider has no tick, no files, no intro and is no module of the count
        if module {
            if tick != 0 {
                crate::entry::publish_tick(rec, tick);
            }
            s.loads = s.loads.wrapping_add(1);
            crate::files::ready(rec);
            crate::intro::commit(rec);
        }
        OK
    }



    /// Undo what `init` and the checks before the commit did for record `rec`; `r` is the result to return (`E_BUSY` when a step of the module is still running).
    unsafe fn fail(&mut self, rec: usize, r: i32) -> i32 {
        let module = rec < MAX_MODULES;
        let mut held = false;
        if module {
            crate::intro::discard(rec);
            held = !crate::files::retire(rec) | !retire_sdk(rec);
        }
        crate::module_cmds::release_cmds(rec as u8 + 1);
        crate::dlog::release_sinks(rec);
        (RoleSteps::<P, M, R>::of(self.roles[rec] as u32).undo)(self, rec);
        self.r.remove(rec);
        if module && !dsp::release(rec) || held {
            return E_BUSY;
        }
        r
    }

    /// A module's file or I/O step still runs, or the DSP loader keeps its rows: the record must not be freed yet. A provider record has none of these.
    unsafe fn cannot_retire(&mut self, rec: usize) -> bool {
        rec < MAX_MODULES && (dsp::retained() || !crate::files::retire(rec) || !retire_sdk(rec))
    }


    /// The commit of 6.3 steps 1 to 3: the declarations of a service file against its header table, the services' part (they check what their builder calls collected, write their
    /// records and declare their DSP rows), the registry walker over the block; the record block stays in the arena.
    unsafe fn commit(&mut self, rec: usize, c: &crate::collect::Collect, keys: &crate::collect::KeysCopy, image_size: u32) -> i32 {
        use crate::collect as col;
        let base = records::module(rec).base;
        // the declarations of a service module against its header table (declare-check-commit)
        if let Some(d) = col::decl_diff(c, keys, base) {
            self.decl_diff = d;
            return E_DECL_KIND;
        }
        // a service file declares its row in `init` (`dk_service_declare_row`); a module declares none. `fail` takes whatever was added down again
        let row = c.decl.row_ptr().map(|p| &*p);
        let r = self.r.add(rec, row);
        if r != OK {
            return r;
        }
        // an error of a generic record (`dk_record_add`); a provider adds no record of its own
        if c.err != 0 {
            return c.err;
        }
        if rec >= MAX_MODULES && c.ng != 0 {
            return E_FORMAT;
        }
        // the services' part of the commit (interface B): they check what their builder calls collected and write their records
        let (baddr, size, rows) = match self.commit_services(rec, c, keys, base, image_size) {
            Ok(v) => v,
            Err(e) => return e,
        };
        if rec < MAX_MODULES {
            col::keep(rec, baddr, size);
        }
        crate::dlog::ev(
            logk::MODULE_COMMIT,
            crate::dlog::INFO,
            [read_volatile(addr_of!(BOOT_IDX)), rows.0 | rows.1 << 8 | rows.2 << 16, baddr, size, 0],
        );
        OK
    }


    /// The commit of the services (`Service::module_commit`): a `PREPARE` pass checks and measures the records, the loader takes one arena block for them, a `WRITE` pass fills it
    /// (the generic records of the module follow the services' records), and the registry walker checks and applies it. Returns the block (address, size; 0, 0 when no record was
    /// written) and the rows the module uses. Nothing stays allocated on an error; the caller's undo gives the services their `remove`.
    unsafe fn commit_services(&mut self, rec: usize, c: &crate::collect::Collect, keys: &crate::collect::KeysCopy, base: u32, image_size: u32) -> Result<(u32, u32, (u32, u32, u32)), i32> {
        use dkrt_abi::service::{commit_phase, ModuleCommitCtx, RecordSink};
        let mut sink = RecordSink::measure();
        let ctx = ModuleCommitCtx { phase: commit_phase::PREPARE, base, image_size, keys, sink: &mut sink };
        let r = self.r.module_commit(rec, &ctx);
        if r != OK {
            return Err(r);
        }
        c.write_generic(&mut sink);
        let (size, count) = (sink.at, sink.count);
        let blk = if size > RecordSink::HEADER {
            match self.m.alloc(size as u32) {
                Some(b) => Some(b),
                None => return Err(E_NOMEM),
            }
        } else {
            None
        };
        let mut out = match blk {
            Some(b) => RecordSink::over(b.ptr, size),
            None => RecordSink::over(core::ptr::null_mut(), 0),
        };
        let ctx = ModuleCommitCtx { phase: commit_phase::WRITE, base, image_size, keys, sink: &mut out };
        let r = self.r.module_commit(rec, &ctx);
        let free = |l: &mut Self| {
            if let Some(b) = blk {
                l.m.free(b.addr);
            }
        };
        if r != OK {
            free(self);
            return Err(r);
        }
        c.write_generic(&mut out);
        let rows = (out.rows[0], out.rows[1], out.rows[2]);
        if let Some(b) = blk {
            if out.at != size || out.count != count {
                free(self);
                return Err(E_FORMAT); // a service wrote another size than it measured
            }
            wr32(b.ptr, 0x444b_5247);
            wr32(b.ptr.add(4), 1);
            wr32(b.ptr.add(8), self.p.device_id());
            wr32(b.ptr.add(12), count);
            // check before anything of the module's rows is declared: a refusal leaves everything as it was
            let r = crate::registry::check(b.ptr, size as u32, image_size, self.p.device_id());
            if r != OK {
                free(self);
                return Err(r);
            }
        }
        let ctx = ModuleCommitCtx { phase: commit_phase::DECLARE, base, image_size, keys, sink: &mut out };
        let r = self.r.module_commit(rec, &ctx);
        if r != OK {
            free(self);
            return Err(r);
        }
        let b = match blk {
            Some(b) => b,
            None => return Ok((0, 0, rows)),
        };
        let r = crate::registry::apply(rec as u8 + 1, b.ptr, size as u32, base);
        if r != OK {
            free(self);
            return Err(r);
        }
        Ok((b.addr, size as u32, rows))
    }

    /// The DSP part of a module published at boot failed (the machine rows and `init` are in already): take it down again.
    pub unsafe fn undo_early(&mut self, rec: usize) -> i32 {
        self.unload_rec(rec)
    }


    /// One UI-loop pass of the DSP loader. `dsp` only advances its state and reports what it found; the module teardown, the machine
    /// listing and the boot result belong here, so `dsp` depends on neither the loader nor the boot.
    pub unsafe fn dsp_step_with(&mut self, l: ServiceTable) {
        // the DSP backend of the installer writes the slots of a service file after its code is acknowledged (`dsp::step_with` asks for them)
        let area = if dsp::have_link() { dsp::area_base() } else { 0 };
        let adv = {
            let di = &mut self.dsp_install;
            dsp::step_with(l, &mut |rec| di.sequence(rec as u8, area))
        };
        match adv {
            dsp::Advance::Pending => {}
            dsp::Advance::Failed(code) => {
                let (rec, idx) = {
                    let p = &dsp::ds().pending;
                    (p.rec as usize, p.boot_idx)
                };
                // Teardown invalidates once, after the tick is stopped, before any fini/free. A service goes the same way, dependents first.
                let reclaimed = self.take_down(rec, code) == OK;
                crate::boot::report_dsp(idx, code);
                dsp::fail_done(reclaimed, code);
            }
            dsp::Advance::Loaded => {
                let (rec, idx) = {
                    let p = &dsp::ds().pending;
                    (p.rec as usize, p.boot_idx)
                };
                l.on_loaded(rec);
                crate::boot::report_dsp(idx, OK);
                dsp::loaded_done();
            }
        }
    }

    /// The DSP part of record `rec` failed with `code`: take the record down. A module goes through `unload_rec`. A service goes through the same `unload_rec`,
    /// after its dependents (the modules and services whose header table needs what it provides): they are failed with `E_MISSING` in their module states, their queued
    /// DSP loads are removed, and each is taken down by `unload_rec` too. The core knows no kind of service here: a need is a service name hash.
    unsafe fn take_down(&mut self, rec: usize, _code: i32) -> i32 {
        if rec >= MAX_MODULES {
            let hash = self.provided_hash(rec - MAX_MODULES);
            let mut d = 0;
            while d < records::REC_MAX && hash != 0 {
                if d != rec && records::module(d).used != 0 && self.needs_hash(d, hash) {
                    // its queued DSP load first: the memory of its image goes back to the arena, the loader never starts it
                    dsp::dequeue(d);
                    let r = self.take_down(d, E_MISSING);
                    if r != OK {
                        return r;
                    }
                    if d < MAX_MODULES {
                        let k = self.sel_of_rec[d];
                        if k != u8::MAX {
                            crate::boot::report_taken_down(k as u32, E_MISSING);
                        }
                    }
                }
                d += 1;
            }
        }
        self.unload_rec(rec)
    }

    /// The hash of the service that provider `j` provides (its PROVIDES item), 0 when unknown.
    unsafe fn provided_hash(&self, j: usize) -> u32 {
        match Keys::parse(self.found.table(j)) {
            Ok(k) => k.words(dkneeds::keys::tag::PROVIDES, 0).map_or(0, |w| w.0),
            Err(_) => 0,
        }
    }

    /// Whether the loaded record `rec` has a hard NEED of the service `hash` (from the header table the boot kept: the selection's for a module, the search's for a provider).
    unsafe fn needs_hash(&self, rec: usize, hash: u32) -> bool {
        let table: &[u8] = if rec < MAX_MODULES {
            let k = self.sel_of_rec[rec] as usize;
            if k >= SEL_MAX || self.table_len[k] == 0 {
                return false;
            }
            &self.tables[k][..self.table_len[k]]
        } else {
            self.found.table(rec - MAX_MODULES)
        };
        let keys = match Keys::parse(table) {
            Ok(k) => k,
            Err(_) => return false,
        };
        let mut i = 0;
        while let Some(w) = keys.words(dkneeds::keys::tag::NEEDS, i) {
            if w.0 == hash && w.2 & 1 != 0 {
                return true;
            }
            i += 1;
        }
        false
    }

    /// Take the install of provider record `rec` out of the image and the DSP: the hook sites back to stock and the ColdFire slot words zero (one mask), then the DSP
    /// slots zero (the same sequence the install sent, with zeros; a slot of the block is zero before the code behind it is freed). False when the DSP does not take the
    /// clear: the caller keeps everything (`E_BUSY`) and the DSP state stays retained.
    unsafe fn uninstall(&mut self, rec: usize) -> bool {
        self.installer.restore(&self.p, rec as u8);
        let area = if dsp::have_link() { dsp::area_base() } else { 0 };
        if let Some(seq) = self.dsp_install.clear_sequence(rec as u8, area) {
            if !crate::link::lk().run(Owner::Loader, &seq) {
                return false;
            }
        }
        self.dsp_install.restore(&self.p, rec as u8);
        true
    }

    /// Take module record `rec` down: its tick, `fini`, machine rows, DSP voice rows and range, memory. Used only
    /// by the DSP loader when the DSP part of a module published at boot fails (`undo_early`). The run-time `unload` that also called
    /// it (with the in-use scan) is archived with hot reload. The tables are checked after it (`invariants::check`).
    pub unsafe fn unload_rec(&mut self, rec: usize) -> i32 {
        let r = self.unload_rec_inner(rec);
        #[cfg(not(test))]
        crate::invariants::check();
        r
    }

    /// The one lifecycle of a loaded record, for modules and services (docs/plans/digikit-hot-reload.md, "Quiesce and unload protocol"): stop what calls into it (the tick
    /// of a module; the hook sites and slots of a service), drain the DSP, `fini`, the role's undo, the registry, the DSP range, the memory.
    unsafe fn unload_rec_inner(&mut self, rec: usize) -> i32 {
        let module = rec < MAX_MODULES;
        if module {
            if !crate::files::retire(rec) { return E_BUSY; }
            if !crate::worker::retire(rec) { return E_BUSY; }
            if !retire_sdk(rec) { return E_BUSY; }
            // an intro of this module that has not run `done` yet: `done` must not point into freed code
            crate::intro::discard(rec);
        }
        let m: &mut Module = records::module(rec);
        if module {
            // 1. clear the tick (one aligned store) so no new call starts; the UI task, where it runs, is this task
            crate::entry::stop_tick(rec);
        } else if !self.uninstall(rec) {
            // 1. a service: nothing may jump into it any more (hook sites back to stock, slots zero)
            return E_BUSY;
        }
        // DSP rows must be invalidated before fini can mutate/release module state.
        if !dsp::quiesce(rec) { return E_BUSY; }
        // 2. fini, 3. free
        if m.fini != 0 {
            let ctx = crate::entry::ctx_addr(rec) as usize as *const Ctx;
            self.m.enter_fini(rec, m.fini, ctx);
        }
        // a sink it set is released with it, with or without a `fini`; nothing may point into its range
        crate::dlog::release_sinks(rec);
        crate::dlog::sink_drop(m.base, m.base.wrapping_add(m.size));
        // the SysEx commands it registered are released with it
        crate::module_cmds::release_cmds(rec as u8 + 1);
        drop_tick_cb(m.base, m.size);
        crate::api::poll_drop_range(m.base, m.size);
        // the machine row first, then the DSP voice rows, then the range in the DSP area
        (RoleSteps::<P, M, R>::of(self.roles[rec] as u32).undo)(self, rec);
        self.r.remove(rec);
        if module {
            let committed = crate::collect::release(rec);
            if committed != 0 {
                self.m.free(committed);
            }
        } else {
            self.loaded[rec - MAX_MODULES] = false;
        }
        dsp::release_range(rec);
        self.m.free(m.base);
        m.used = 0;
        records::set_menu(rec, 0);
        crate::entry::unbind(rec);
        OK
    }

    /// Keep the header table of the `.dkm` `file` (the whole file in the staging buffer) for selection index `k`.
    pub unsafe fn capture(&mut self, k: usize, file: &[u8]) {
        if k >= SEL_MAX || file.len() < dkneeds::HEADER as usize {
            return;
        }
        let be = |at: usize| u32::from_be_bytes([file[at], file[at + 1], file[at + 2], file[at + 3]]) as usize;
        let (off, len) = (be(dkneeds::keys::H_KEYS_OFF as usize), be(dkneeds::keys::H_KEYS_SIZE as usize));
        if len == 0 || len > TABLE_MAX || off > file.len() || len > file.len() - off {
            return;
        }
        self.tables[k][..len].copy_from_slice(&file[off..off + len]);
        self.table_len[k] = len;
    }

    /// Forget the tables (the start of a boot's planning).
    pub fn reset_plan(&mut self) {
        self.table_len = [0; SEL_MAX];
        self.planned_dsp = (0, 0);
    }

    /// Plan the selection `count` modules long from the tables `capture` kept. The hard needs that the runtime does not meet are looked up as `services/<name>.dks`
    /// (`discover::find`, one listing, only when there is such a need); the providers found are planned with the modules and load before them.
    #[inline(never)]
    pub unsafe fn plan_with_caps(&mut self, count: usize, caps: &[DkrtCap]) -> Plan {
        let n = count.min(SEL_MAX);
        let mut mods: [Option<Keys>; SEL_MAX] = [None; SEL_MAX];
        let mut i = 0;
        while i < n {
            if self.table_len[i] != 0 {
                mods[i] = Keys::parse(&self.tables[i][..self.table_len[i]]).ok();
            }
            i += 1;
        }
        // the hard needs that no service of the runtime meets
        self.found.n = 0;
        let mut want = [0u32; WANT_MAX];
        let nw = unmet_needs(&mods[..n], &mut want);
        if nw != 0 {
            discover::find(&mut self.m, &services(), &want[..nw], &mut self.found);
        }
        let provs = parse_provs(&self.found, self.found.n);
        let env = Env { services: services(), runtime: dkrt_sdk::RUNTIME_VERSION, caps };
        let p = plan::plan_with(&env, &mods[..n], &provs[..self.found.n]);
        self.planned_dsp = p.dsp_load;
        p
    }

    /// The provider stage of the boot: each provider of the plan in plan order, before any module. A provider the plan refused is not loaded; a provider that
    /// fails (its own provider did not load: E_IMPORT) undoes itself. The result goes to the log (`boot::report_provider`); the module states do not
    /// change (`LOADS` and `DkrtBootInfo` know no provider).
    #[inline(never)]
    pub unsafe fn load_providers(&mut self, planned: &Plan) {
        self.phase = Phase::Boot;
        self.loaded = [false; PROVIDER_MAX];
        crate::selection::BOOT_IDX = PROVIDER_IDX;
        let mut hashes = [0u32; PROVIDER_MAX];
        {
            let provs = parse_provs(&self.found, planned.np);
            let mut j = 0;
            while j < planned.np {
                if let Some(t) = &provs[j] {
                    hashes[j] = t.words(dkneeds::keys::tag::PROVIDES, 0).map_or(0, |w| w.0);
                }
                j += 1;
            }
        }
        let mut pos = 0;
        while pos < planned.np {
            let j = planned.provider_order[pos] as usize;
            pos += 1;
            self.missing = [0; NEEDS_LEN];
            self.decl_diff = (0, 0);
            let res = match planned.provider_verdicts[j] {
                dkrt_abi::plan::Verdict::Refused(why) => why.code(),
                dkrt_abi::plan::Verdict::Accepted => {
                    // a provider whose own provider did not load fails at its imports (E_IMPORT)
                    let mut path = [0u8; discover::PATH_CAP + 1];
                    let at = self.found.path(j);
                    path[..at.len()].copy_from_slice(at);
                    self.load_provider(path.as_ptr(), j)
                }
            };
            // a provider whose DSP part follows (`LOADING`) is live on the ColdFire from here: its dependents load, and a failure of the DSP part takes them down again
            self.loaded[j] = res == OK || res == LOADING;
            let base = if res == OK || res == LOADING { records::module(MAX_MODULES + j).base } else { 0 };
            // a refusal of the declare-check names the item that differs, in the words of the base (the tag) and the service hash (the item's first word)
            let (hash, base) = if res == E_DECL_KIND && self.decl_diff.0 != 0 { (self.decl_diff.1, self.decl_diff.0) } else { (hashes[j], base) };
            crate::boot::report_provider(self.found.path(j), res, hash, base);
        }
        crate::selection::BOOT_IDX = u32::MAX;
        self.phase = Phase::Closed;
    }

    /// The settle step: the modules that need a provider that did not load are refused before the module stage starts (`plan::settle`).
    #[inline(never)]
    pub unsafe fn settle(&mut self, planned: &mut Plan, count: usize) {
        let n = count.min(SEL_MAX);
        let mut mods: [Option<Keys>; SEL_MAX] = [None; SEL_MAX];
        let mut i = 0;
        while i < n {
            if self.table_len[i] != 0 {
                mods[i] = Keys::parse(&self.tables[i][..self.table_len[i]]).ok();
            }
            i += 1;
        }
        let provs = parse_provs(&self.found, planned.np);
        plan::settle(&services(), planned, &mods[..n], &provs[..planned.np], &self.loaded[..planned.np]);
        // every service of the boot is loaded and no module yet: the services plan what they keep per selected module
        let sel = settle_ctx(planned, &self.tables, &self.table_len, n);
        services().settle(&sel);
    }
}

/// What `settle` hands to the services: the keys of the modules the plan accepted. A module the plan refused (a repeated key, no room, a missing need) gives none, so
/// it takes no number and a key that it repeats never reaches a service's assignment.
#[inline(never)]
pub(crate) fn settle_ctx(planned: &Plan, tables: &[[u8; TABLE_MAX]; SEL_MAX], lens: &[usize; SEL_MAX], n: usize) -> dkrt_abi::service::SettleCtx {
    let mut sel = dkrt_abi::service::SettleCtx::new();
    sel.count = n;
    let mut i = 0;
    while i < n {
        if planned.verdicts[i].is_accepted() {
            sel.nkeys[i] = table_keys(&tables[i][..lens[i]], &mut sel.keys[i]) as u8;
        }
        i += 1;
    }
    sel
}

/// The keys of a module's header table that `settle` hands to the services (`table` empty: none), at most 8; their count.
#[inline(never)]
pub(crate) fn table_keys(table: &[u8], out: &mut [u32; 8]) -> usize {
    let k = match dkrt_abi::keys::KeysCopy::from_bytes(table) {
        Ok(k) => k,
        Err(_) => return 0,
    };
    let n = k.nm.min(8);
    out[..n].copy_from_slice(&k.mk[..n]);
    n
}

/// The header tables of the first `n` providers the search found.
#[inline(never)]
fn parse_provs(found: &Found, n: usize) -> [Option<Keys<'_>>; PROVIDER_MAX] {
    let mut provs: [Option<Keys>; PROVIDER_MAX] = [None; PROVIDER_MAX];
    let mut j = 0;
    while j < n.min(PROVIDER_MAX) {
        provs[j] = Keys::parse(found.table(j)).ok();
        j += 1;
    }
    provs
}

/// Most hard needs the search is asked for: one boot's distinct needs of services that the runtime does not provide.
const WANT_MAX: usize = 24;

/// The distinct hard needs of `mods` that no service of the runtime meets, into `out`; their count.
#[inline(never)]
fn unmet_needs(mods: &[Option<Keys>], out: &mut [u32; WANT_MAX]) -> usize {
    let mut n = 0;
    let mut m = 0;
    while m < mods.len() {
        if let Some(t) = &mods[m] {
            let mut i = 0;
            while let Some(w) = t.words(dkneeds::keys::tag::NEEDS, i) {
                i += 1;
                if w.2 & 1 == 0 || plan::is_runtime_service(w.0, &services()) {
                    continue;
                }
                let mut seen = false;
                let mut k = 0;
                while k < n {
                    seen |= out[k] == w.0;
                    k += 1;
                }
                if !seen && n < WANT_MAX {
                    out[n] = w.0;
                    n += 1;
                }
            }
        }
        m += 1;
    }
    n
}

/// The provider of the last `_open` stub that answered DK_E_MISSING since the clear before `init` (`stub_missing`), NUL padded; empty when none.
/// When `init` returns DK_E_MISSING, `Loader::missing` takes it and the load fails with E_IMPORT, as for an unresolved import.
static mut CALLED_NEED: [u8; NEEDS_LEN] = [0; NEEDS_LEN];

/// Called by the stub of an absent service's `_open`: records `provider` (`core/usb-bulk` becomes `usb-bulk`) as the need, replacing an earlier one.
pub fn stub_missing(provider: &'static str) {
    let n = match provider.rfind('/') {
        Some(at) => &provider[at + 1..],
        None => provider,
    };
    let b = n.as_bytes();
    unsafe {
        CALLED_NEED = [0; NEEDS_LEN];
        let mut k = 0;
        while k < b.len() && k < NEEDS_LEN {
            CALLED_NEED[k] = b[k];
            k += 1;
        }
    }
}

extern "C" {
    /// `core/dsp-link`: clears the per-block callback when it points into [lo, hi). Resolves to 0 when the module is not in the image.
    fn dl_tick_cb_drop();
}

/// After a module's `fini`: no per-block callback of `core/dsp-link` may point into its memory (a module that forgot to clear it).
unsafe fn drop_tick_cb(base: u32, size: u32) {
    let a = core::hint::black_box(dl_tick_cb_drop as unsafe extern "C" fn() as usize);
    if a != 0 {
        let f: unsafe extern "C" fn(u32, u32) = core::mem::transmute(a);
        f(base, base.wrapping_add(size));
    }
}

/// The module `rec` is going away (an unload or a rollback): its I/O jobs end without `done`, and a file it was writing is dropped. False: an
/// I/O step of it is running (it is cancelled); the caller keeps the module's memory (`E_BUSY`) until a later call returns true.
pub(crate) unsafe fn retire_sdk(rec: usize) -> bool {
    if !crate::io::retire(rec) {
        return false;
    }
    crate::store_writer::retire(rec as u8 + 1);
    true
}

