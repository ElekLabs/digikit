//! The `.dkm` loader (docs/design/digikit-dkm.md). Every check runs before anything is written to the arena.

use crate::dsp;
use crate::entry::Ctx;
use crate::admission::Admission;
use discover::Found;
use dkrt_abi::plan::Verdict;
use crate::handlers::{services, ServiceTable};
use dkneeds::keys::Keys;
use install::{Backend, CfItems, DspInstaller, Installer, SITES_MAX};
use role::{PlaceArgs, RoleSteps};
use exports::Exports;
use crate::platform::RealPlatform;
use crate::selection::BOOT_IDX;
use crate::{cstr_eq, cstr_len, rd32, st, wr32, Module, MAX_MODULES};
use core::ptr::{addr_of, addr_of_mut, read_volatile};
use dkrt_abi::link::Owner;
use dkrt_abi::package::PackageId;
use dkrt_abi::platform::Platform;
use dkrt_sdk::*;

/// The most bytes of a header table the loader keeps for the planning of a boot (a module with eight machines, three effects and nine needs has about 260; a larger table is not
/// planned and the load decides). A provider file's table without its FW, FWNAMES and SITES entries holds its hooks and exports too: `machines.dks` with its builder exports needs
/// about 480.
pub(crate) const TABLE_MAX: usize = dkneeds::keys::BOOT_TABLE_MAX;

/// `init`, `fini`, `menu` and `tick` are image offsets in the header (NONE for a missing one): the fields of the module's `dk_module` constant
/// (`dk_module_def`), called with a `dk_module_ctx *` (`entry.rs`).
const NONE: u32 = dkneeds::NONE;

// header field offsets (big endian): `[format]` of sdk/sdk.toml; the parse in `image.rs` reads the rest
const H_VERSION: usize = dkneeds::H_VERSION as usize;
const H_NAME: usize = dkneeds::H_NAME as usize;
/// Bytes of a module's name in a log event (three words).
pub const LOG_NAME: usize = 12;

/// The name part of the module id in `head` (the NAME_LEN bytes of the header's name): after the last `/`, cut to `LOG_NAME`, NUL padded.
fn name_part(head: &[u8]) -> [u8; LOG_NAME] {
    let n = head.iter().position(|b| *b == 0).unwrap_or(head.len());
    let id = &head[..n];
    let from = id.iter().rposition(|b| *b == b'/').map_or(0, |i| i + 1);
    let mut out = [0u8; LOG_NAME];
    let name = &id[from..];
    let m = name.len().min(LOG_NAME);
    out[..m].copy_from_slice(&name[..m]);
    out
}
const H_REG_OFF: usize = dkneeds::H_REG_OFF as usize;
const H_REG_SIZE: usize = dkneeds::H_REG_SIZE as usize;
const H_DSP_OFF: usize = dkneeds::H_DSP_OFF as usize;
const H_DSP_SIZE: usize = dkneeds::H_DSP_SIZE as usize;

pub(crate) mod discover;
pub(crate) mod exports;
mod image;
pub(crate) mod install;
mod live;
pub(crate) mod package;
pub(crate) use package::PhaseKind;
mod port;
pub(crate) mod records;
pub(crate) mod role;
mod seams;
#[cfg(test)]
pub(crate) mod tests;
use image::in_file;
pub use seams::{Block, Images, RealImages, RealRegistry, Registry, Staged};

/// The index the log shows for a provider file (`MOD_LOAD` and the events of its load): not a selection index, and not `u32::MAX` (no load running).
pub const PROVIDER_IDX: u32 = 0xffff_fffe;

/// What a provider record keeps after its load started: the copy of its header table (HOOKS, EXPORTS and SLOTS; for the port file the whole table; the staged file is gone once `init` reads
/// from the store) and the size of its image.
#[derive(Clone, Copy)]
pub(crate) struct ProvRec {
    pub block: Option<Block>,
    pub len: u32,
    pub image_size: u32,
    /// The PROVIDES identity and optional callback order, read before the staged bytes can change.
    pub provides: Option<(u32, u32)>,
    pub callback_order: Option<u32>,
    /// The activated, resident-only v1 query entry. Zero when absent or retired.
    query_v1: u32,
    /// Bytes of the DSP image of the file (0: it has no DSP section): the bound of the entries of its DSP slot fills.
    pub dsp_image: u32,
}

impl ProvRec {
    pub const EMPTY: ProvRec = ProvRec { block: None, len: 0, image_size: 0, provides: None, callback_order: None, query_v1: 0, dsp_image: 0 };
}

/// Which loads the loader accepts. The boot hook loads the module set; after it only a live job loads (docs/plans/extension-model.md E5c).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// The state after reset and after the boot's loop: `load` is `E_BOOT_ONLY`.
    Closed = 0,
    /// While the boot hook loads the selection: the CF part of a module with DSP code is published at once (the machine view is built soon after the
    /// hook, and the frames that carry DSP code do not flow yet); the DSP part follows over the UI-loop passes.
    Boot = 1,
    /// While a live job loads a module file: a reload loads the new file into the record of the old module (`Loader::reload_load`), a load takes the first free record
    /// (`Loader::live_load`). Nothing else loads in this phase.
    Live = 2,
}

/// Where a load puts the file: the first free module record, a module's own record (a reload loads into the record of the module it replaces, so the
/// owner ids in the machine and DSP rows do not move), or a provider record (a service file).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Slot {
    Free,
    Module(usize),
    Provider(usize),
}

impl Slot {
    /// The fixed record, if the slot names one.
    fn fixed(self) -> Option<usize> {
        match self {
            Slot::Free => None,
            Slot::Module(r) | Slot::Provider(r) => Some(r),
        }
    }
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
    /// The role word of each record's file (0 module, 1 service, 2 port): which `RoleSteps` its undo uses.
    roles: [u8; records::REC_MAX],
    /// The name of the missing module of the last load that failed with E_IMPORT (`import_needs`), NUL padded; empty when none. The boot clears it before
    /// each load and takes it after.
    missing: [u8; NEEDS_LEN],
    /// The first item that `init`'s declarations and the header table do not both hold, of the last load that failed with E_DECL_KIND: (tag of the item, its first
    /// word), and 0x100 added to the tag when only the header has it. `(0, 0)`: none (`report_provider` logs it).
    decl_diff: (u32, u32),
    /// The name hash that the port file did not list, of the last service file that failed with E_IMPORT for it (`port::Lookup::Unlisted`); 0: none.
    import_hash: u32,
    /// The header table of the module at each selection index, kept while the boot reads each file once (`capture`); `table_len` 0: none (the file
    /// is missing, torn, of the old format or has a larger table).
    tables: [[u8; TABLE_MAX]; SEL_MAX],
    table_len: [usize; SEL_MAX],
    /// Whether the module at each selection index stays (set by `number_boot_keys`, and by a live load): `keys_of` gives the keys of such a module only, so a module that
    /// was refused (a repeated key, no room, a missing need) takes no number from a service.
    accepted: [bool; SEL_MAX],
    /// The DSP load of the packages admitted so far: (load, largest per-voice cost), which `dkrt_capacity` reports as used and largest of
    /// `CAPK_DSP_COST`; the module set is fixed per session, so the admission is the load.
    planned_dsp: (u32, u32),
    /// The providers the boot's search found: their paths and tables. Provider `j` is record `MAX_MODULES + j`.
    found: Found,
    /// The name part of the module id (after the `/`, 12 bytes: the log's width, NUL padded) of the file at each selection index, kept by `capture` for a plan refusal; all zero: no header was read.
    log_name: [[u8; LOG_NAME]; SEL_MAX],
    /// The same for the last file `check_file` read a header of (zero when the last check did not get that far).
    last_name: [u8; LOG_NAME],
    /// The providers the boot tried and refused (the admission, or the manager's bring-up): the live check does not count them. A provider that is only not up (nothing needed it, or
    /// the manager took it down when its last consumer went) is not here: a live load brings it up again.
    unusable: [bool; PROVIDER_MAX],
    /// The table copy of each provider record (`role.rs`).
    provs: [ProvRec; PROVIDER_MAX],
    /// The functions the loaded providers offer by name; an import that the runtime's table lacks resolves here.
    exports: Exports,
    /// The hook sites and slot words of the loaded providers in the image (the ColdFire backend). Not armed in safe mode.
    installer: Installer,
    /// The slots of the providers' DSP code in core/dsp-link's slot block (the DSP backend).
    dsp_install: DspInstaller,
    /// The selection index of each module record (`u8::MAX`: none): the late teardown of a service names the dependents' states by it.
    /// Stored as the index plus 1, so that 0 (none) is the zero fill of the static: a non-zero initial value would put the whole loader in `.data` (read it with `sel_of`).
    sel_of_rec: [u8; MAX_MODULES],
    /// The steps of a teardown that ran for each record (`TORN_*`): a second `unload_body` after `E_BUSY` skips them, so `fini` and the role's undo run once.
    torn: [u8; records::REC_MAX],
    /// The tick address of a module whose tick the teardown stopped (valid while `TORN_TICK` is set): a reload that cannot finish gives it back.
    saved_tick: [u32; MAX_MODULES],
    /// The block count when a reload closed the entries of each module (`TORN_DRAIN`); `None`: the platform has no block clock.
    drain_at: [Option<u32>; MAX_MODULES],
    /// A package the package manager starts applies the records of the services in the table first and leaves those of the loaded service files to `attach`
    /// (`package_attach`): set for the duration of `package_start`.
    split_apply: bool,
    /// The block of each module's records that `attach` applies (pointer and size), kept from the commit to the take-down.
    reg_block: [(*const u8, u32); MAX_MODULES],
}

/// Bits of `Loader::torn`.
const TORN_TICK: u8 = 1;
const TORN_FINI: u8 = 2;
const TORN_UNDO: u8 = 4;
/// The entries of the module are closed and the wait of a reload has started (`drain_at`).
const TORN_DRAIN: u8 = 8;

/// The loader of the device.
pub(crate) type Real = Loader<RealPlatform, RealImages, RealRegistry>;

static mut LOADER: Real = Loader::new(RealPlatform, RealImages, RealRegistry);

fn loader() -> &'static mut Real {
    unsafe { &mut *addr_of_mut!(LOADER) }
}

/// The loader of the device, for the package manager's session (`lifecycle`).
pub(crate) fn real() -> *mut Real {
    addr_of_mut!(LOADER)
}

/// The address a loaded service file published for the export `hash`, if any (`handlers::exports::find_export`).
pub fn find_export(hash: u32) -> Option<u32> {
    loader().exports.resolve(hash)
}

/// Copy the query address out of the loader before entering provider code. Both run on the UI task.
pub fn resident_query(provider: u32, minimum: u32) -> Result<u32, i32> {
    loader().resident_query(provider, minimum)
}

/// The name part of the module id of the last file whose header a check read (zero: none); a live or reload job reports it.
pub fn last_name() -> [u8; LOG_NAME] {
    loader().last_name
}

/// Validate and load the `.dkm` at `path`. See `dkrt_load`.
pub unsafe fn load(path: *const u8) -> i32 {
    loader().load(path)
}

/// Safe mode: the installer writes nothing.
pub unsafe fn set_safe() {
    loader().installer.set_armed(false)
}

/// The checks of a reload of module record `rec` by the file at `path`, before anything is unloaded (`Loader::reload_check`): the file, its keys, and the plan of
/// the live set with the file in the old module's slot. `OK` or the refusal.
pub unsafe fn reload_check(rec: usize, path: *const u8) -> i32 {
    let mut caps = [DkrtCap { kind: 0, limit: 0, used: 0, largest: 0 }; CAP_MAX];
    let k = live_capacity(&mut caps, own_use(rec));
    loader().reload_check(rec, path, &caps[..k])
}

/// The capacity as a plan of the whole live set needs it (`dkrt_capacity` into `caps` through `live_caps`, with `own` given back); its count.
pub(crate) unsafe fn live_capacity(caps: &mut [DkrtCap; CAP_MAX], own: (u32, u32)) -> usize {
    let k = crate::dkrt_capacity(caps.as_mut_ptr(), CAP_MAX as u32) as usize;
    live_caps(&mut caps[..k], own);
    k
}

/// The first step of a reload's teardown (`Loader::reload_quiesce`): `OK` when nothing is inside the module any more, `E_BUSY` while the caller must ask again.
pub unsafe fn reload_quiesce(rec: usize) -> i32 {
    loader().reload_quiesce(rec)
}

/// The module record loaded from the store path `path` (NUL terminated), and its selection index: what a live job names its module by.
pub unsafe fn rec_of_path(path: *const u8) -> Option<(usize, usize)> {
    let l = loader();
    let mut rec = 0;
    while rec < MAX_MODULES {
        if records::module(rec).used != 0 && cstr_eq(records::path(rec).as_ptr(), path) {
            let k = l.sel_of(rec) as usize;
            return if k < SEL_MAX { Some((rec, k)) } else { None };
        }
        rec += 1;
    }
    None
}

/// Does the DSP loader hold a module it could not release? Permanent until the next boot: every load and reload is refused.
pub unsafe fn dsp_retained() -> bool {
    dsp::retained()
}

/// How far the DSP loader has got with its loads: a counter that moves when an image or a row goes out or a load starts.
pub unsafe fn dsp_progress() -> u32 {
    let d = dsp::ds();
    d.step.wrapping_add(d.transfers).wrapping_add(d.row_writes)
}

/// Is `path` the store path of a loaded provider (a service file)?
pub unsafe fn is_provider_path(path: *const u8) -> bool {
    let mut j = 0;
    while j < PROVIDER_MAX {
        let rec = MAX_MODULES + j;
        if records::module(rec).used != 0 && cstr_eq(records::path(rec).as_ptr(), path) {
            return true;
        }
        j += 1;
    }
    false
}

/// A reload that cannot finish before the old code is gone: give the old module its tick back (`Loader::reload_abort`).
pub unsafe fn reload_abort(rec: usize) -> bool {
    loader().reload_abort(rec)
}

/// The name of the missing module of the last load that failed with `E_IMPORT` (`Loader::missing`).
pub(crate) fn missing() -> [u8; NEEDS_LEN] {
    loader().missing()
}

/// The selection indices at which a module is loaded.
pub(crate) unsafe fn live_indices() -> [bool; SEL_MAX] {
    loader().live_at(SEL_MAX)
}

/// The checks of a load of the file at `path` into selection index `k` (`Loader::live_check_load`).
pub unsafe fn live_check_load(k: usize, path: *const u8) -> i32 {
    let mut caps = [DkrtCap { kind: 0, limit: 0, used: 0, largest: 0 }; CAP_MAX];
    let n = live_capacity(&mut caps, (0, 0));
    loader().live_check_load(k, path, &caps[..n])
}

/// Selection index `k` has no module any more (`Loader::retire_index`).
pub unsafe fn retire_index(k: usize) {
    loader().retire_index(k)
}

/// What module record `rec` holds that its reload gives back before the new file loads: (arena bytes of its image block and its record block, DSP area bytes of its
/// range).
pub(crate) unsafe fn own_use(rec: usize) -> (u32, u32) {
    if rec >= MAX_MODULES {
        return (0, 0);
    }
    let block = |n: u32| if n == 0 { 0 } else { dkneeds::arena_block(n).unwrap_or(n) };
    let m = records::module(rec);
    (block(m.size) + block(crate::collect::block_of(rec).1), dsp::ds().mod_len[rec])
}

/// The capacity as a plan of the whole live set needs it: what the live modules use is counted by the plan again, so the counts they own go back to 0. The
/// physical resources (arena, staging buffer, DSP area, store, settings) keep their use, less `own` (the arena and DSP bytes of the module being reloaded, which its
/// teardown frees before the new file loads, so a large module is not refused for counting itself twice). The plan still counts the record block of every other live
/// module again, which errs on the side of refusing.
pub(crate) fn live_caps(caps: &mut [DkrtCap], own: (u32, u32)) {
    use dkrt_sdk::proto::*;
    let mut c = 0;
    while c < caps.len() {
        let kind = caps[c].kind;
        let physical = kind == CAPK_ARENA as u32 || kind == CAPK_STAGE as u32 || kind == CAPK_DSP_AREA as u32 || kind == CAPK_STORE as u32 || kind == CAPK_SETTINGS as u32;
        if kind == CAPK_ARENA as u32 {
            caps[c].used = caps[c].used.saturating_sub(own.0);
        } else if kind == CAPK_DSP_AREA as u32 {
            caps[c].used = caps[c].used.saturating_sub(own.1);
        }
        if !physical {
            caps[c].used = 0;
            caps[c].largest = if kind == CAPK_SELECTION as u32 { caps[c].limit } else { 0 };
        }
        c += 1;
    }
}

/// Keep the header table of the `.dkm` `file` (the whole file in the staging buffer) for selection index `k`.
pub unsafe fn capture(k: usize, file: &[u8]) {
    loader().capture(k, file)
}

/// The keys of record kind `kind` of the accepted module at selection index `at` (`CoreApi::keys_of`). A read of the loader's tables, which `settle` and the loads that
/// follow do not change.
pub fn keys_of(at: u32, kind: u32, out: &mut [u32; 8]) -> usize {
    loader().keys_of(at, kind, out)
}

/// A host test's boot: the module at selection index `at` has the header table `table` and the plan accepted it.
#[cfg(test)]
pub(crate) fn test_accept(at: usize, table: &[u8]) {
    let l = loader();
    l.tables[at][..table.len()].copy_from_slice(table);
    l.table_len[at] = table.len();
    l.accepted[at] = true;
}

/// A host test seeds a record by hand: no teardown of an earlier test is half done.
#[cfg(test)]
pub(crate) unsafe fn test_clear_torn() {
    loader().torn = [0; records::REC_MAX];
}

/// Forget the tables (the start of a boot's planning).
pub unsafe fn reset_plan() {
    loader().reset_plan()
}

/// The capacity as the boot's admission starts with it (`dkrt_capacity`, into `caps`; its count). The report counts the selection's paths as used from the start of the boot
/// (`boot::selected`), and the admission counts them one by one as it takes the modules; and the admission starts with no DSP load (the report counts its own result from its end).
pub(crate) unsafe fn boot_caps(caps: &mut [DkrtCap; CAP_MAX]) -> usize {
    let k = crate::dkrt_capacity(caps.as_mut_ptr(), CAP_MAX as u32) as usize;
    let mut c = 0;
    while c < k {
        if caps[c].kind == dkrt_sdk::proto::CAPK_SELECTION as u32 {
            caps[c].used = 0;
            caps[c].largest = caps[c].limit;
        }
        if caps[c].kind == dkrt_sdk::proto::CAPK_DSP_COST as u32 {
            caps[c].used = 0;
            caps[c].largest = 0;
        }
        c += 1;
    }
    k
}

/// Search the card for the services the selection `count` modules long needs, from the tables `capture` kept (`Loader::find_providers`).
pub unsafe fn find_providers(count: usize) {
    loader().find_providers(count)
}

/// The DSP load of the last plan: (load, largest per-voice cost). A read of the field that takes no reference: module code can reach it (`dkrt_capacity`) while
/// a load runs.
pub unsafe fn planned_dsp() -> (u32, u32) {
    *addr_of!(LOADER.planned_dsp)
}

/// What `check_file` found in a file, for the load that follows (or for a reload's checks, which only read it). The pointers are into the staging buffer.
struct Checked {
    f: *const u8,
    size: u32,
    head: image::Head,
    role: u32,
    /// The address of each import of the file (the runtime's table, or a provider's export).
    import_addrs: [u32; dkneeds::MAX_CF_IMPORTS as usize],
    reg_off: u32,
    reg_size: u32,
    keys: crate::collect::KeysCopy,
    dsp_size: u32,
    job: dsp::Job,
}

/// What `place_image` leaves for the steps that follow: the arguments of the role's place step and the addresses of `init` and `tick` (0: none).
struct Placed {
    place: PlaceArgs,
    init: u32,
    tick: u32,
}

impl Checked {
    const fn empty() -> Checked {
        Checked {
            f: core::ptr::null(),
            size: 0,
            head: image::Head { needs: 0, image_off: 0, image_size: 0, bss: 0, reloc_off: 0, reloc_n: 0, import_off: 0, import_n: 0, init: 0, fini: 0, menu: 0, tick: 0 },
            role: 0,
            import_addrs: [0; dkneeds::MAX_CF_IMPORTS as usize],
            reg_off: 0,
            reg_size: 0,
            keys: crate::collect::KeysCopy::empty(),
            dsp_size: 0,
            job: dsp::Job::empty(),
        }
    }
}

impl<P: Platform, M: Images, R: Registry> Loader<P, M, R> {
    /// The selection index of module record `rec`, `u8::MAX` when it has none.
    fn sel_of(&self, rec: usize) -> u8 {
        self.sel_of_rec[rec].wrapping_sub(1)
    }

    pub const fn new(p: P, m: M, r: R) -> Self {
        Loader {
            p,
            m,
            r,
            phase: Phase::Closed,
            roles: [0; records::REC_MAX],
            missing: [0; NEEDS_LEN],
            decl_diff: (0, 0),
            import_hash: 0,
            tables: [[0; TABLE_MAX]; SEL_MAX],
            table_len: [0; SEL_MAX],
            accepted: [false; SEL_MAX],
            planned_dsp: (0, 0),
            found: Found::new(),
            log_name: [[0; LOG_NAME]; SEL_MAX],
            last_name: [0; LOG_NAME],
            unusable: [false; PROVIDER_MAX],
            provs: [ProvRec::EMPTY; PROVIDER_MAX],
            exports: Exports::new(),
            installer: Installer::new(),
            dsp_install: DspInstaller::new(),
            sel_of_rec: [0; MAX_MODULES],
            torn: [0; records::REC_MAX],
            saved_tick: [0; MAX_MODULES],
            drain_at: [None; MAX_MODULES],
            split_apply: false,
            reg_block: [(core::ptr::null(), 0); MAX_MODULES],
        }
    }

    /// The port file that is loaded: its record, the copy of its table and the base and image size of its block. `None` when no port file is loaded.
    unsafe fn port_keys(&self) -> Option<(Keys<'static>, u32, u32)> {
        let mut j = 0;
        while j < PROVIDER_MAX {
            let rec = MAX_MODULES + j;
            if self.roles[rec] == 2 && records::module(rec).used != 0 {
                let pr = &self.provs[j];
                let blk = pr.block?;
                let keys = Keys::parse(core::slice::from_raw_parts(blk.ptr as *const u8, pr.len as usize)).ok()?;
                return Some((keys, records::module(rec).base, pr.image_size));
            }
            j += 1;
        }
        None
    }

    /// Resolve the import `name` of a service file against the loaded port file. A name that is no firmware reference is `NotPort`; a firmware reference with no port
    /// file loaded is `Unlisted` (the hash is the reference's), as a name the port file does not list.
    unsafe fn port_lookup(&self, name: &[u8]) -> port::Lookup {
        let Some(r) = port::parse_import(name) else {
            return port::Lookup::NotPort;
        };
        match self.port_keys() {
            Some((keys, base, image_size)) => port::lookup(&keys, base, image_size, r),
            None => port::Lookup::Unlisted(r.hash),
        }
    }

    /// The name of the missing module of the last load that failed with `E_IMPORT`, NUL padded; empty when none.
    pub fn missing(&self) -> [u8; NEEDS_LEN] {
        self.missing
    }

    /// Validate and load the `.dkm` at `path`. The tables are checked after every attempt, and none starts once a check has failed (`invariants::check`).
    pub unsafe fn load(&mut self, path: *const u8) -> i32 {
        self.load_file(path, Slot::Free)
    }

    unsafe fn load_file(&mut self, path: *const u8, slot: Slot) -> i32 {
        if crate::invariants::bad() {
            return E_TABLE;
        }
        let r = self.load_inner(path, slot);
        #[cfg(not(test))]
        crate::invariants::check();
        r
    }

    /// The checks of a file: everything that can be refused before a record or a byte of the arena is taken. The staged file stays in the staging buffer (`c.f`)
    /// until the next `stage`. `slot` says which role the file must have: a service file only for a provider record, a module file only for the others.
    /// Three parts in this order: the header and the role's check (`check_head`), the imports (`resolve_imports`), the sections (`check_body`).
    unsafe fn check_file(&mut self, path: *const u8, slot: Slot, c: &mut Checked) -> i32 {
        let r = self.check_head(path, slot, c);
        if r != OK {
            return r;
        }
        let r = self.resolve_imports(c, None);
        if r != OK {
            return r;
        }
        self.check_body(c)
    }

    /// The first part of `check_file`: stage the file, parse its header, the role and the role's own checks.
    unsafe fn check_head(&mut self, path: *const u8, slot: Slot, c: &mut Checked) -> i32 {
        let staged = match self.m.stage(path) {
            Ok(s) => s,
            Err(e) => return e,
        };
        let f = staged.ptr;
        let size = staged.size;
        c.f = f;
        c.size = size;
        self.last_name = [0; LOG_NAME];

        // ---- checks (nothing is written outside the staging buffer) ----
        let head = match image::head(f, size) {
            Ok(h) => h,
            Err(e) => return e,
        };
        c.head = head;
        // the header is read: the log names the module by its id, not by its file
        self.last_name = name_part(core::slice::from_raw_parts(f.add(H_NAME), dkneeds::NAME_LEN as usize));
        // the steps of the file's role
        let role = rd32(f.add(dkneeds::H_ROLE as usize));
        c.role = role;
        if (role == 1 || role == 2) != matches!(slot, Slot::Provider(_)) {
            return E_FORMAT;
        }
        let steps = RoleSteps::<P, M, R>::of(role);
        (steps.check)(self, f, &head)
    }

    /// The second part of `check_file`: the address of every import of the staged file, into `c.import_addrs`. An explicit service import names its provider; legacy
    /// SDK imports keep the global lookup. `needs`: when given, an import that resolves to the export of a loaded provider resolves only if that provider's record is one of
    /// `needs` (the services of the package's needs, `PackageId` is the record); the runtime's table, the service-only imports and the loaded port file are not
    /// bound by it.
    unsafe fn resolve_imports(&mut self, c: &mut Checked, needs: Option<&[PackageId]>) -> i32 {
        let (f, size, role) = (c.f, c.size, c.role);
        let image::Head { import_off, import_n, .. } = c.head;
        let allowed = |rec: usize| needs.map_or(true, |n| n.iter().any(|p| p.0 as usize == rec));
        // An explicit service import names its provider; legacy SDK imports keep the global lookup.
        let (reg_off, reg_size) = (rd32(f.add(H_REG_OFF)), rd32(f.add(H_REG_SIZE)));
        let import_owners = if import_n != 0 && in_file(reg_off, reg_size, size) {
            Keys::parse(core::slice::from_raw_parts(f.add(reg_off as usize), reg_size as usize)).ok()
        } else {
            None
        };
        let mut i = 0;
        while i < import_n as usize {
            let p = f.add(import_off as usize + i * dkneeds::CF_IMPORT_LEN as usize);
            let n = cstr_len(p, dkneeds::CF_IMPORT_LEN as usize);
            if n == dkneeds::CF_IMPORT_LEN as usize {
                return E_FORMAT;
            }
            let name = core::slice::from_raw_parts(p, n);
            let name_hash = dkneeds::keys::name_hash_bytes(name);
            let mut a = self.m.import_addr(p);
            if a == 0 && role == 1 {
                // a service file also links the service-only imports (`SVC_IMPORTS`)
                a = self.m.svc_import_addr(p);
            }
            if let Some(keys) = import_owners {
                let service = match service_import_owner(keys, name_hash) {
                    Ok(s) => s,
                    Err(e) => return e,
                };
                if let Some((service, minimum)) = service {
                    if a != 0 || port::parse_import(name).is_some() {
                        return E_IMPORT;
                    }
                    match self.exports.owned(name_hash) {
                        Some((addr, rec)) => {
                            if let Some((owner, version)) = self.provs[rec - MAX_MODULES].provides {
                                if allowed(rec) && owner == service && version >= minimum && version >> 16 == minimum >> 16 {
                                    c.import_addrs[i] = addr;
                                    i += 1;
                                    continue;
                                }
                            }
                        }
                        None => {}
                    }
                    return E_IMPORT;
                }
            }
            if a == 0 && role == 1 {
                // the firmware references of a service file are the port file's: an address, 0 for a name that this firmware lacks, or a refusal
                let found = self.port_lookup(name);
                if let port::Lookup::Addr(x) = found {
                    a = x;
                } else if found == port::Lookup::Absent {
                    c.import_addrs[i] = 0;
                    i += 1;
                    continue;
                } else if let port::Lookup::Unlisted(h) = found {
                    self.import_hash = h;
                    return E_IMPORT;
                }
            }
            if a == 0 {
                // the runtime's table has no such name: a loaded provider may export it
                a = match self.exports.owned(name_hash) {
                    Some((addr, rec)) if allowed(rec) => self.exports.resolve(name_hash).map_or(0, |_| addr),
                    _ => 0,
                };
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
            c.import_addrs[i] = a;
            i += 1;
        }
        OK
    }

    /// The third part of `check_file`: the relocation table, the `register` table and the DSP section.
    unsafe fn check_body(&mut self, c: &mut Checked) -> i32 {
        let (f, size, role, head) = (c.f, c.size, c.role, c.head);
        let (reg_off, reg_size) = (rd32(f.add(H_REG_OFF)), rd32(f.add(H_REG_SIZE)));
        // relocations: kind, bounds, import index
        if let Err(e) = image::sections(f, &head) {
            return e;
        }
        // `register` records (machines, parameters): validated now, written after the copy
        c.reg_off = reg_off;
        c.reg_size = reg_size;
        if !in_file(reg_off, reg_size, size) {
            return E_FORMAT;
        }
        let reg = f.add(reg_off as usize);
        // the table is the keys table; the records come from the builder calls of `init` (complete)
        c.keys = match crate::collect::KeysCopy::from_bytes(core::slice::from_raw_parts(reg, reg_size as usize)) {
            Ok(k) => k,
            Err(e) => return e,
        };
        if role == 1 {
            role::filter_hooks(self, &mut c.keys);
        }
        // the DSP section, if any
        let (dsp_off, dsp_size) = (rd32(f.add(H_DSP_OFF)), rd32(f.add(H_DSP_SIZE)));
        c.dsp_size = dsp_size;
        if dsp_size != 0 {
            let r = dsp::check(f, dsp_off, dsp_size, size, &mut c.job);
            if r != OK {
                return r;
            }
            let (v, stride) = (c.job.voices, c.job.vstride as usize);
            let r = crate::collect::check_dsp_keys(&c.keys, c.job.nvoice as usize, |i| rd32(v.add(stride * i)), c.job.nfx as usize);
            if r != OK {
                return r;
            }
        }
        OK
    }

    /// `slot`: where the file goes (`Slot`). A service file loads only into a provider record, and a module file never does.
    unsafe fn load_inner(&mut self, path: *const u8, slot: Slot) -> i32 {
        dsp::init_once();
        // the boot loads the selection; after it only a live job loads: into the record of the module a reload replaces, or into a free record
        if !matches!((self.phase, slot), (Phase::Boot, _) | (Phase::Live, Slot::Module(_) | Slot::Free)) {
            return E_BOOT_ONLY;
        }
        let mut c = Checked::empty();
        let r = self.check_file(path, slot, &mut c);
        if r != OK {
            return r;
        }
        let (f, role) = (c.f, c.role);
        let image::Head { image_size, bss, .. } = c.head;
        let steps = RoleSteps::<P, M, R>::of(role);
        let name = f.add(H_NAME);
        let total = image_size + bss;
        let dsp_size = c.dsp_size;
        let job = &c.job;
        // already loaded; a free record
        let rec = match self.claim_record(slot, name) {
            Ok(rec) => rec,
            Err(e) => return e,
        };

        self.torn[rec] = 0;

        // ---- allocate, copy, relocate, zero bss ----
        // The DSP range, the relocated DSP image and the voice rows come first: nothing else is allocated yet if they fail.
        if dsp_size != 0 {
            let r = dsp::reserve(rec, job);
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
        let base = blk.addr;
        let Placed { place, init: initaddr, tick: tickaddr } = self.place_image(rec, path, &c, blk, image::Pass::Both);
        let image::Head { image_size, .. } = c.head;
        let keys = &c.keys;
        let m: &mut Module = records::module(rec);
        if dsp_size != 0 {
            // the CF part goes live now (rows in place before the machine view is built); the DSP code and the voice rows follow
            // when the frames flow
            m.used = 1;
            let r = self.complete(rec, keys, initaddr, tickaddr, image_size, steps, place);
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
        let r = self.complete(rec, keys, initaddr, tickaddr, image_size, steps, place);
        if r != OK {
            if self.cannot_retire(rec) { return E_BUSY; }
            self.m.free(base);
            m.used = 0;
            crate::entry::unbind(rec);
        }
        r
    }

    /// The record a file named `name` loads into: the slot's own, which must be free (a reload puts the same name back; another name is another module, which the name
    /// rule checks against the other records), or the first free module record. `E_LOADED`, or the refusal of `capacity::pick_record`.
    unsafe fn claim_record(&self, slot: Slot, name: *const u8) -> Result<usize, i32> {
        let s = st();
        match slot.fixed() {
            Some(rec) => {
                if records::module(rec).used != 0 {
                    return Err(E_LOADED);
                }
                if let Slot::Module(_) = slot {
                    let mut k = 0;
                    while k < MAX_MODULES {
                        if k != rec && s.modules[k].used != 0 && cstr_eq(s.modules[k].name.as_ptr(), name) {
                            return Err(E_LOADED);
                        }
                        k += 1;
                    }
                }
                Ok(rec)
            }
            None => capacity::pick_record(MAX_MODULES, |i| s.modules[i].used != 0, |i| cstr_eq(s.modules[i].name.as_ptr(), name)),
        }
    }

    /// The image of the checked file `c` into block `blk` (`total` bytes) of record `rec`: copy, zero the bss, the relocations of `pass` (`image::relocate`; the import
    /// addresses of `c` are those of `Both` and `Imports`), the caches, then the record: its name, path, version, base, size, `fini`, entry binding and menu. The record is
    /// in use afterwards.
    unsafe fn place_image(&mut self, rec: usize, path: *const u8, c: &Checked, blk: Block, pass: image::Pass) -> Placed {
        let (f, role) = (c.f, c.role);
        let image::Head { needs, image_size, bss, init, fini, menu, tick, .. } = c.head;
        let name = f.add(H_NAME);
        let total = image_size + bss;
        let (mem, base) = (blk.ptr, blk.addr);
        image::copy(mem, f, &c.head);
        image::relocate(mem, f, &c.head, base, &c.import_addrs, pass);
        // ---- caches: write the lines back and drop them from both caches ----
        self.p.flush(base, total);

        // ---- record, init, publish ----
        self.roles[rec] = if role == 2 { 2 } else { (role == 1) as u8 };
        if rec < MAX_MODULES {
            let k = read_volatile(addr_of!(BOOT_IDX));
            self.sel_of_rec[rec] = if (k as usize) < SEL_MAX { k as u8 + 1 } else { 0 };
        }
        let m: &mut Module = records::module(rec);
        m.used = 1;
        let mut i = 0;
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
        let place = PlaceArgs { file: f, keys_off: c.reg_off, keys_len: c.reg_size, image_size, dsp_image: if c.dsp_size != 0 { c.job.image_size } else { 0 }, mem, base, bss };
        Placed { place, init: if init == NONE { 0 } else { base + init }, tick: if tick == NONE { 0 } else { base + tick } }
    }

    /// The end of a load, shared by the immediate and the DSP path (both run in the boot hook; docs/plans/sdk.md 6.3): `init` runs first and its builder calls are collected; then the commit checks them, fills the
    /// voice rows' ranges, writes the registry rows and keeps the record block. Anything that fails undoes what `init` did and writes no row.
    #[allow(clippy::too_many_arguments)]
    unsafe fn complete(&mut self, rec: usize, keys: &crate::collect::KeysCopy, init: u32, tick: u32, image_size: u32, steps: RoleSteps<P, M, R>, place: PlaceArgs) -> i32 {
        let r = self.init_and_commit(rec, keys, init, image_size, steps, place);
        if r != OK {
            return r;
        }
        let r = (steps.activate)(self, rec);
        if r != OK {
            return self.fail(rec, r);
        }
        self.go_live(rec, tick);
        OK
    }

    /// The first part of `complete`: the role's place step, `init` with the builder calls collected, then the commit. A failure undoes what was done (`fail`) and is the result.
    #[allow(clippy::too_many_arguments)]
    unsafe fn init_and_commit(&mut self, rec: usize, keys: &crate::collect::KeysCopy, init: u32, image_size: u32, steps: RoleSteps<P, M, R>, place: PlaceArgs) -> i32 {
        // the role's part of the file that `init` could overwrite in the staging buffer
        let r = (steps.place)(self, rec, &place);
        if r != OK {
            return self.fail(rec, r);
        }
        // the listing client and the I/O jobs are for every record: a service file uses the SDK file calls in its `init` as a module does
        crate::files::init_begin(rec);
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
        OK
    }

    /// The last part of `complete`: the record is live. A service file has no intro and is no module of the count; its tick runs as a module's does.
    unsafe fn go_live(&mut self, rec: usize, tick: u32) {
        let s = st();
        let module = rec < MAX_MODULES;
        let m: &mut Module = records::module(rec);
        m.used = 1;
        if tick != 0 {
            crate::entry::publish_tick(rec, tick);
        }
        crate::files::ready(rec);
        if module {
            s.loads = s.loads.wrapping_add(1);
            crate::intro::commit(rec);
        }
    }

    /// Undo what `init` and the checks before the commit did for record `rec`; `r` is the result to return (`E_BUSY` when a step of the module is still running).
    unsafe fn fail(&mut self, rec: usize, r: i32) -> i32 {
        let module = rec < MAX_MODULES;
        if module {
            crate::intro::discard(rec);
        }
        let held = !crate::files::retire(rec) | !retire_sdk(rec);
        crate::module_cmds::release_cmds(rec as u8 + 1);
        crate::dlog::release_sinks(rec);
        (RoleSteps::<P, M, R>::of(self.roles[rec] as u32).undo)(self, rec);
        self.r.remove(rec);
        if module && !dsp::release(rec) || held {
            return E_BUSY;
        }
        r
    }

    /// A file or I/O step of the record still runs, or the DSP loader keeps a module's rows: the record must not be freed yet.
    unsafe fn cannot_retire(&mut self, rec: usize) -> bool {
        (rec < MAX_MODULES && dsp::retained()) || !crate::files::retire(rec) || !retire_sdk(rec)
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
        let mut order = None;
        if let Some(row) = row {
            if rec < MAX_MODULES {
                return E_DECL_KIND;
            }
            let provider = &self.provs[rec - MAX_MODULES];
            if provider.provides.map(|p| p.0) != Some(dkneeds::keys::name_hash(row.name())) {
                return E_DECL_KIND;
            }
            order = provider.callback_order;
        }
        let r = self.r.add(rec, row, order);
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
        let r = if self.split_apply && rec < MAX_MODULES {
            // the records are applied when the module is attached, over the rows of its services in one walk (`package_attach`)
            self.reg_block[rec] = (b.ptr as *const u8, size as u32);
            OK
        } else {
            crate::registry::apply(rec as u8 + 1, b.ptr, size as u32, base)
        };
        if r != OK {
            free(self);
            return Err(r);
        }
        Ok((b.addr, size as u32, rows))
    }


    /// One pass of the DSP state machine (`dsp::step_with`), with the DSP backend of the installer writing the slots of a service file once its code is acknowledged.
    pub(crate) unsafe fn dsp_advance(&mut self, l: ServiceTable) -> dsp::Advance {
        let area = if dsp::have_link() { dsp::area_base() } else { 0 };
        let di = &mut self.dsp_install;
        dsp::step_with(l, &mut |rec| di.sequence(rec as u8, area))
    }

    /// Every row of the load in flight is in: the services hear it, the boot record lists the module, the next load starts.
    pub(crate) unsafe fn dsp_loaded(&mut self, l: ServiceTable) {
        let (rec, idx) = {
            let p = &dsp::ds().pending;
            (p.rec as usize, p.boot_idx)
        };
        l.on_loaded(rec);
        crate::boot::report_dsp(idx, OK);
        dsp::loaded_done();
    }

    /// The hash of the service that provider `j` provides (its PROVIDES item), 0 when unknown.
    pub(crate) unsafe fn provided_hash(&self, j: usize) -> u32 {
        match Keys::parse(self.found.table(j)) {
            Ok(k) => k.words(dkneeds::keys::tag::PROVIDES, 0).map_or(0, |w| w.0),
            Err(_) => 0,
        }
    }

    /// The ColdFire part of `uninstall`: the hook sites back to stock and the slot words zero.
    unsafe fn uninstall_cf(&mut self, rec: usize) {
        self.installer.restore(&self.p, rec as u8);
    }

    /// The DSP part of `uninstall`: the DSP slots zero. False when the DSP does not take the clear.
    unsafe fn uninstall_dsp(&mut self, rec: usize) -> bool {
        let area = if dsp::have_link() { dsp::area_base() } else { 0 };
        if let Some(seq) = self.dsp_install.clear_sequence(rec as u8, area) {
            if !crate::link::lk().run(Owner::Loader, &seq) {
                return false;
            }
        }
        self.dsp_install.restore(&self.p, rec as u8);
        true
    }

    /// Stop the tick of module record `rec` once; the address is kept for a reload that gives up.
    unsafe fn stop_tick_once(&mut self, rec: usize) {
        if self.torn[rec] & TORN_TICK == 0 {
            self.saved_tick[rec] = crate::entry::tick_of(rec);
            crate::entry::stop_tick(rec);
            self.torn[rec] |= TORN_TICK;
        }
    }

    /// The start of a service's take-down: no new tick call and no resident query entry. The package manager restores the entries (hook sites, slots) itself, after this and before
    /// it stops the service.
    unsafe fn quiesce_service(&mut self, rec: usize) {
        crate::entry::stop_tick(rec);
        self.provs[rec - MAX_MODULES].query_v1 = 0;
    }

    /// The rest of the take-down of record `rec`, after a service's `quiesce_service`.
    unsafe fn unload_body(&mut self, rec: usize) -> i32 {
        let module = rec < MAX_MODULES;
        // the listing client and the I/O jobs of a module or a service file go before its code does; a queued job's `done` must not point into freed code
        if !crate::files::retire(rec) { return E_BUSY; }
        if !crate::worker::retire(rec) { return E_BUSY; }
        if !retire_sdk(rec) { return E_BUSY; }
        if module {
            // an intro of this module that has not run `done` yet: `done` must not point into freed code
            crate::intro::discard(rec);
            // the record's next module has opened no settings
            crate::settings_reload::note_unloaded(rec);
        }
        let m: &mut Module = records::module(rec);
        if module {
            // 1. clear the tick (one aligned store) so no new call starts; the UI task, where it runs, is this task. The address is kept for a reload that gives up.
            self.stop_tick_once(rec);
        }
        // DSP rows must be invalidated before fini can mutate/release module state.
        if !dsp::quiesce(rec) { return E_BUSY; }
        // 2. fini, 3. free
        if m.fini != 0 && self.torn[rec] & TORN_FINI == 0 {
            // from here the old code is gone for good: a reload cannot give the tick back
            self.torn[rec] |= TORN_FINI;
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
        // the unload observers end what they hold for the module (`dkrt_unload_observer_add`): the code and memory are still there, but nothing may call into the module
        if module {
            crate::api::notify_unload(rec, m.base, m.size);
        }
        // the machine row first, then the DSP voice rows, then the range in the DSP area
        if self.torn[rec] & TORN_UNDO == 0 {
            self.torn[rec] |= TORN_UNDO;
            (RoleSteps::<P, M, R>::of(self.roles[rec] as u32).undo)(self, rec);
            self.r.remove(rec);
        }
        if module {
            self.reg_block[rec] = (core::ptr::null(), 0);
            let committed = crate::collect::release(rec);
            if committed != 0 {
                self.m.free(committed);
            }
        }
        dsp::release_range(rec);
        self.m.free(m.base);
        m.used = 0;
        records::set_menu(rec, 0);
        crate::entry::unbind(rec);
        self.torn[rec] = 0;
        if module {
            let b = crate::boot::info();
            b.unloads = b.unloads.wrapping_add(1);
        }
        OK
    }

    fn resident_query(&self, provider: u32, minimum: u32) -> Result<u32, i32> {
        if self.phase != Phase::Closed {
            return Err(E_CONTEXT);
        }
        for record in self.provs.iter() {
            // the entry is published from the activation of the package to its deactivation: a provider with one is up
            if record.query_v1 == 0 {
                continue;
            }
            if let Some((name, version)) = record.provides {
                if name == provider {
                    return if version >= minimum && version >> 16 == minimum >> 16 {
                        Ok(record.query_v1)
                    } else {
                        Err(E_CONTRACT)
                    };
                }
            }
        }
        Err(E_MISSING)
    }

    /// The checks of a reload of module record `rec` by the file at `path`, before anything is unloaded: nothing changes but the staging buffer. In this order: the
    /// record is a live module of the boot selection; the file passes every check of a load (`check_file`), has the same name, a header table the planner can keep, and
    /// every key of the old file (`E_RELOAD_KEYS`); the planner accepts the live set with the file in the old module's slot (`caps`: `live_caps`). `OK`, or the refusal.
    ///
    /// The checks of a file read the numbers the services gave its keys (`resolve` of the machines service names a voice's type by the selection index of the module that loads,
    /// `CoreApi::boot_index`). A load has that index set and the keys numbered by `settle`; so the check sets the index to the module's, and numbers the file's keys first (the old
    /// keys keep their numbers, a key the file adds takes a free one, as `reload_load` does again). The numbering is done only for a file that has the module's name and every old key.
    pub unsafe fn reload_check(&mut self, rec: usize, path: *const u8, caps: &[DkrtCap]) -> i32 {
        if rec >= MAX_MODULES || records::module(rec).used == 0 {
            return E_NOT_FOUND;
        }
        let k = self.sel_of(rec) as usize;
        if k >= SEL_MAX || self.table_len[k] == 0 {
            return E_NOT_FOUND;
        }
        if let Ok(staged) = self.m.stage(path) {
            let file = core::slice::from_raw_parts(staged.ptr, staged.size as usize);
            if let Some(table) = table_of_file(file) {
                let same_name = file.len() >= H_NAME + dkneeds::NAME_LEN as usize && file[H_NAME..H_NAME + dkneeds::NAME_LEN as usize] == records::module(rec).name[..];
                if let (true, Ok(old), Ok(new)) = (same_name, Keys::parse(&self.tables[k][..self.table_len[k]]), Keys::parse(table)) {
                    if old.first_missing_key(&new).is_none() {
                        self.number_keys(k, file);
                    }
                }
            }
        }
        let was = read_volatile(addr_of!(BOOT_IDX));
        crate::selection::BOOT_IDX = k as u32;
        let r = self.reload_check_inner(rec, k, path, caps);
        crate::selection::BOOT_IDX = was;
        r
    }

    unsafe fn reload_check_inner(&mut self, rec: usize, k: usize, path: *const u8, caps: &[DkrtCap]) -> i32 {
        let mut c = Checked::empty();
        let r = self.check_file(path, Slot::Module(rec), &mut c);
        if r != OK {
            return r;
        }
        if !cstr_eq(c.f.add(H_NAME), records::module(rec).name.as_ptr()) {
            return E_FORMAT; // another name is another module
        }
        if c.reg_size as usize > TABLE_MAX {
            return E_TOO_BIG;
        }
        let new = match Keys::parse(core::slice::from_raw_parts(c.f.add(c.reg_off as usize), c.reg_size as usize)) {
            Ok(t) => t,
            Err(_) => return E_FORMAT,
        };
        match Keys::parse(&self.tables[k][..self.table_len[k]]) {
            Ok(old) => {
                if old.first_missing_key(&new).is_some() {
                    return E_RELOAD_KEYS;
                }
            }
            Err(_) => return E_NOT_FOUND,
        }
        self.live_plan(k, core::slice::from_raw_parts(c.f.add(c.reg_off as usize), c.reg_size as usize), caps)
    }

    /// The first step of a reload's teardown, called once per pass until it returns `OK`: close every way into the module's code, then wait until nothing is inside.
    /// The tick stops; its SysEx commands answer `E_BUSY` from now on; its sinks, with the span clock, are cleared (under the interrupt mask); the block count is
    /// noted. It returns `E_BUSY` until no command of the module is in flight and `DRAIN_BLOCKS` blocks have passed (no spinning: the next UI pass asks again).
    /// Nothing is freed and no DSP row is touched, so `reload_abort` can still give the module back its tick and commands. `E_NOT_FOUND`: no such module.
    ///
    /// The channel sinks (`dlog::CHAN`) are cleared and an abort does not set them again: a module that owned one (the debug module's log sink) loses it until the
    /// next boot. The legacy single sink and tick words of `dlog` (`SINK`, `TICK`) are not touched here: they point into the module's code, which stays mapped until
    /// `unload_body` frees it, and `dlog::sink_drop` clears them (masked) before that free, so a task that calls them in between runs code that is still there.
    pub unsafe fn reload_quiesce(&mut self, rec: usize) -> i32 {
        if rec >= MAX_MODULES || records::module(rec).used == 0 {
            return E_NOT_FOUND;
        }
        let owner = rec as u8 + 1;
        if self.torn[rec] & TORN_DRAIN == 0 {
            self.stop_tick_once(rec);
            crate::module_cmds::set_draining(owner, true);
            crate::dlog::release_sinks(rec);
            self.drain_at[rec] = self.p.blocks();
            self.torn[rec] |= TORN_DRAIN;
        }
        if crate::module_cmds::in_flight(owner) != 0 || !crate::reload::drained(&self.p, self.drain_at[rec]) {
            return E_BUSY;
        }
        OK
    }

    /// The teardown of a reload cannot go on (the DSP did not drain in time): if `fini` has not run, the old module gets its tick back and the reload ends with
    /// the old module live. `false` when the old code is gone (`fini` ran): the caller finishes the teardown.
    pub unsafe fn reload_abort(&mut self, rec: usize) -> bool {
        if rec >= MAX_MODULES || self.torn[rec] & TORN_FINI != 0 {
            return false;
        }
        if self.torn[rec] & TORN_TICK != 0 {
            crate::entry::publish_tick(rec, self.saved_tick[rec]);
        }
        crate::module_cmds::set_draining(rec as u8 + 1, false);
        self.torn[rec] = 0;
        true
    }

    /// The part of `reload_load` before the file loads: the rules of the check for the bytes that load, the file's header table kept at the module's index, the
    /// services' numbering. The package manager loads the file after it.
    pub(crate) unsafe fn reload_prepare(&mut self, rec: usize, path: *const u8) -> i32 {
        let k = if rec < MAX_MODULES { self.sel_of(rec) as usize } else { SEL_MAX };
        if k >= SEL_MAX {
            return E_NOT_FOUND;
        }
        // the file can have changed since `reload_check` (a host writes it while the module drains): the rules of the check hold for the bytes that load
        let staged = match self.m.stage(path) {
            Ok(s) => s,
            Err(e) => return e,
        };
        let file = core::slice::from_raw_parts(staged.ptr, staged.size as usize);
        if file.len() < dkneeds::HEADER as usize || file[H_NAME..H_NAME + dkneeds::NAME_LEN as usize] != records::module(rec).name[..] {
            return E_FORMAT;
        }
        let (off, len) = match table_span(file) {
            Some(s) => s,
            None => return E_FORMAT,
        };
        if len > TABLE_MAX {
            return E_TOO_BIG;
        }
        match (Keys::parse(&self.tables[k][..self.table_len[k]]), Keys::parse(&file[off..off + len])) {
            (Ok(old), Ok(new)) => {
                if old.first_missing_key(&new).is_some() {
                    return E_RELOAD_KEYS;
                }
            }
            (_, Err(_)) => return E_FORMAT,
            (Err(_), _) => return E_NOT_FOUND,
        }
        self.capture(k, file);
        self.accepted[k] = true;
        // the services number the live set again: the live modules keep their numbers (sticky), a key the new file adds takes a free one
        let sel = live_settle_ctx(&self.tables, &self.table_len, &self.live_at(k));
        services().settle(&sel);
        OK
    }

    /// The services number the keys of `file` as if it were the module at selection index `k` (`settle` over the live set with its table there); the kept table of `k` is as it was after.
    #[inline(never)]
    unsafe fn number_keys(&mut self, k: usize, file: &[u8]) {
        let saved = (self.tables[k], self.table_len[k]);
        self.capture(k, file);
        let sel = live_settle_ctx(&self.tables, &self.table_len, &self.live_at(k));
        services().settle(&sel);
        self.tables[k] = saved.0;
        self.table_len[k] = saved.1;
    }

    /// The checks of a load of the file at `path` into selection index `k` (`boot::live_index`), before anything is loaded: nothing changes but the staging buffer. The file
    /// passes every check of a load (`check_file`, as a free record), no loaded module has its name (`E_LOADED`), and the planner accepts the live set with the file at `k`
    /// (`caps`: `live_caps` with `own = (0, 0)`, since nothing is given back). The services number the file's keys first, as a reload's check does, because the checks of the
    /// DSP section read the numbers by the selection index; the kept table of the index is as it was after. `OK`, or the refusal.
    pub unsafe fn live_check_load(&mut self, k: usize, path: *const u8, caps: &[DkrtCap]) -> i32 {
        if k >= SEL_MAX {
            return E_SEL_COUNT;
        }
        if let Ok(staged) = self.m.stage(path) {
            let file = core::slice::from_raw_parts(staged.ptr, staged.size as usize);
            if table_of_file(file).is_some() {
                self.number_keys(k, file);
            }
        }
        let was = read_volatile(addr_of!(BOOT_IDX));
        crate::selection::BOOT_IDX = k as u32;
        self.missing = [0; NEEDS_LEN];
        let r = self.live_check_load_inner(k, path, caps);
        crate::selection::BOOT_IDX = was;
        r
    }

    unsafe fn live_check_load_inner(&mut self, k: usize, path: *const u8, caps: &[DkrtCap]) -> i32 {
        // the plan first, as the boot does (it plans every file before it loads one): a service that no loaded provider offers is `E_MISSING` before the file's imports
        // are looked up, where it would be `E_IMPORT` for the first export of that service. A file whose table cannot be read goes on to `check_file`, which says why.
        if let Ok(staged) = self.m.stage(path) {
            let file = core::slice::from_raw_parts(staged.ptr, staged.size as usize);
            // a loaded name is its own answer: its keys would clash with the loaded module's, which the plan would call `E_PLAN_KEY`
            if file.len() >= H_NAME + dkneeds::NAME_LEN as usize {
                let mut rec = 0;
                while rec < MAX_MODULES {
                    if records::module(rec).used != 0 && records::module(rec).name[..] == file[H_NAME..H_NAME + dkneeds::NAME_LEN as usize] {
                        return E_LOADED;
                    }
                    rec += 1;
                }
            }
            if let Some(table) = table_of_file(file) {
                if Keys::parse(table).is_ok() {
                    let r = self.live_plan(k, table, caps);
                    if r != OK {
                        return r;
                    }
                }
            }
        }
        let mut c = Checked::empty();
        let r = self.check_file(path, Slot::Free, &mut c);
        if r != OK {
            return r;
        }
        let mut rec = 0;
        while rec < MAX_MODULES {
            if records::module(rec).used != 0 && cstr_eq(records::module(rec).name.as_ptr(), c.f.add(H_NAME)) {
                return E_LOADED;
            }
            rec += 1;
        }
        if c.reg_size as usize > TABLE_MAX {
            return E_TOO_BIG;
        }
        if Keys::parse(core::slice::from_raw_parts(c.f.add(c.reg_off as usize), c.reg_size as usize)).is_err() {
            return E_FORMAT;
        }
        OK
    }

    /// The part of `live_load` before the file loads (see `reload_prepare`): the file's header table kept at index `k`, the services' numbering.
    pub(crate) unsafe fn live_prepare(&mut self, k: usize, path: *const u8) -> i32 {
        if k >= SEL_MAX {
            return E_SEL_COUNT;
        }
        // the file can have changed since `live_check_load`: the rules of the check hold for the bytes that load
        let staged = match self.m.stage(path) {
            Ok(s) => s,
            Err(e) => return e,
        };
        let file = core::slice::from_raw_parts(staged.ptr, staged.size as usize);
        let (off, len) = match table_span(file) {
            Some(s) => s,
            None => return E_FORMAT,
        };
        if len > TABLE_MAX {
            return E_TOO_BIG;
        }
        if Keys::parse(&file[off..off + len]).is_err() {
            return E_FORMAT;
        }
        self.capture(k, file);
        self.accepted[k] = true;
        let sel = live_settle_ctx(&self.tables, &self.table_len, &self.live_at(k));
        services().settle(&sel);
        OK
    }

    /// Selection index `k` has no module any more (an unload, or a load that failed): the plan's acceptance goes, and no free record keeps the index. The kept header table
    /// stays (nothing reads it without a live record), and so do the services' sticky numbers, so a later load of the same file gets the same numbers.
    pub unsafe fn retire_index(&mut self, k: usize) {
        if k >= SEL_MAX {
            return;
        }
        self.accepted[k] = false;
        let mut rec = 0;
        while rec < MAX_MODULES {
            if self.sel_of(rec) as usize == k && records::module(rec).used == 0 {
                self.sel_of_rec[rec] = 0;
            }
            rec += 1;
        }
    }

    /// Keep the header table of the `.dkm` `file` (the whole file in the staging buffer) for selection index `k`.
    pub unsafe fn capture(&mut self, k: usize, file: &[u8]) {
        if k >= SEL_MAX || file.len() < dkneeds::HEADER as usize {
            return;
        }
        self.log_name[k] = name_part(&file[H_NAME..H_NAME + dkneeds::NAME_LEN as usize]);
        let be = |at: usize| u32::from_be_bytes([file[at], file[at + 1], file[at + 2], file[at + 3]]) as usize;
        let (off, len) = (be(dkneeds::keys::H_KEYS_OFF as usize), be(dkneeds::keys::H_KEYS_SIZE as usize));
        if len == 0 || len > TABLE_MAX || off > file.len() || len > file.len() - off {
            return;
        }
        self.tables[k][..len].copy_from_slice(&file[off..off + len]);
        self.table_len[k] = len;
    }

    /// The keys of record kind `kind` in the header table of the accepted module at selection index `at` (`CoreApi::keys_of`).
    pub fn keys_of(&self, at: u32, kind: u32, out: &mut [u32; 8]) -> usize {
        let k = at as usize;
        if k >= SEL_MAX || !self.accepted[k] {
            return 0;
        }
        dkrt_abi::keys::keys_of(&self.tables[k][..self.table_len[k]], kind, out)
    }

    /// Forget the tables (the start of a boot's planning).
    pub fn reset_plan(&mut self) {
        self.table_len = [0; SEL_MAX];
        self.accepted = [false; SEL_MAX];
        self.planned_dsp = (0, 0);
    }

    /// Search the card for the services the selection `count` modules long needs, from the tables `capture` kept. The hard needs that the runtime does not meet are looked up as
    /// `services/<name>.dks` (`discover::find`, one listing, only when there is such a need); the providers found are the package graph's services, and the manager brings
    /// them up before the modules that need them.
    #[inline(never)]
    pub unsafe fn find_providers(&mut self, count: usize) {
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
            discover::find(&mut self.m, &services(), &want[..nw], role::image_fw_id(&self.p), &mut self.found);
            // a limit of the search was passed: a need may be unmet for that reason, not for a missing file; each cause has its own name word (`(tab`, `(fil`, `(nee`) and its `TRUNC_*` bit as the hash word, which `loadFailureText` of runtime-log.ts turns into text
            let t = self.found.truncated;
            if t & discover::TRUNC_TABLE != 0 {
                crate::boot::report_provider(b"(tab", E_PLAN_FIT, discover::TRUNC_TABLE as u32, 0);
            }
            if t & discover::TRUNC_FILES != 0 {
                crate::boot::report_provider(b"(fil", E_PLAN_FIT, discover::TRUNC_FILES as u32, 0);
            }
            if t & discover::TRUNC_NEEDS != 0 {
                crate::boot::report_provider(b"(nee", E_PLAN_FIT, discover::TRUNC_NEEDS as u32, 0);
            }
        }
    }

    /// The judgment of package `id` by the rules of the core (`Admission`): a module against the providers and the modules admitted before it, a provider against the providers.
    /// The DSP load the admitted packages left is what `dkrt_capacity` reports (`planned_dsp`).
    pub(crate) unsafe fn admit_package(&mut self, adm: &mut Admission, id: PackageId) -> Verdict {
        let mut mods: [Option<Keys>; SEL_MAX] = [None; SEL_MAX];
        let mut i = 0;
        while i < SEL_MAX {
            if self.table_len[i] != 0 {
                mods[i] = Keys::parse(&self.tables[i][..self.table_len[i]]).ok();
            }
            i += 1;
        }
        let provs = parse_provs(&self.found, self.found.n);
        let rec = id.0 as usize;
        let v = if rec < MAX_MODULES {
            adm.judge_module(&services(), &mods, &provs[..self.found.n.min(PROVIDER_MAX)], rec)
        } else {
            adm.judge_provider(&services(), &provs[..self.found.n.min(PROVIDER_MAX)], rec - MAX_MODULES)
        };
        self.planned_dsp = adm.dsp_load();
        v
    }

    /// The services plan what they keep per selected module, once every service of the boot has been tried: `admitted[k]` says that the module at selection index `k` was admitted and
    /// every service it needs is up, so a module that is refused (a repeated key, no room, a missing need) takes no number and a key that it repeats never reaches a service's
    /// assignment. `keys_of` gives the keys of such a module only.
    #[inline(never)]
    pub unsafe fn number_boot_keys(&mut self, admitted: &[bool; SEL_MAX], count: usize) {
        let n = count.min(SEL_MAX);
        let mut a = 0;
        while a < SEL_MAX {
            self.accepted[a] = a < n && admitted[a];
            a += 1;
        }
        let sel = settle_ctx(&self.accepted, &self.tables, &self.table_len, n);
        services().settle(&sel);
    }

}

/// What `number_keys` hands to the services: the keys of the modules in `accepted`. A module that was refused gives none.
#[inline(never)]
pub(crate) fn settle_ctx(accepted: &[bool; SEL_MAX], tables: &[[u8; TABLE_MAX]; SEL_MAX], lens: &[usize; SEL_MAX], n: usize) -> dkrt_abi::service::SettleCtx {
    let mut sel = dkrt_abi::service::SettleCtx::new();
    sel.count = n;
    let mut i = 0;
    while i < n {
        if accepted[i] {
            sel.nkeys[i] = table_keys(&tables[i][..lens[i]], &mut sel.keys[i]) as u8;
        }
        i += 1;
    }
    sel
}

/// The header table of a `.dkm`: its offset and length in `file`, `None` when the header is short, the table empty or outside the file.
fn table_span(file: &[u8]) -> Option<(usize, usize)> {
    if file.len() < dkneeds::HEADER as usize {
        return None;
    }
    let be = |at: usize| u32::from_be_bytes([file[at], file[at + 1], file[at + 2], file[at + 3]]) as usize;
    let (off, len) = (be(dkneeds::keys::H_KEYS_OFF as usize), be(dkneeds::keys::H_KEYS_SIZE as usize));
    if len == 0 || off > file.len() || len > file.len() - off {
        None
    } else {
        Some((off, len))
    }
}

/// The header table of `file` when it fits the planner's limit.
fn table_of_file(file: &[u8]) -> Option<&[u8]> {
    let (off, len) = table_span(file)?;
    if len > TABLE_MAX {
        None
    } else {
        Some(&file[off..off + len])
    }
}

/// What a reload's `settle` hands to the services: the keys of the modules in `live` (selection indices), in selection order. The count reaches the last live one.
#[inline(never)]
pub(crate) fn live_settle_ctx(tables: &[[u8; TABLE_MAX]; SEL_MAX], lens: &[usize; SEL_MAX], live: &[bool; SEL_MAX]) -> dkrt_abi::service::SettleCtx {
    let mut sel = dkrt_abi::service::SettleCtx::new();
    let mut i = 0;
    while i < SEL_MAX {
        if live[i] {
            sel.count = i + 1;
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
fn service_import_owner(keys: Keys<'_>, name: u32) -> Result<Option<(u32, u32)>, i32> {
    use dkneeds::keys::tag;
    let mut i = 0;
    while let Some((hash, service, _)) = keys.words(tag::SERVICE_IMPORTS, i) {
        if hash == name {
            let mut j = 0;
            while let Some((needed, version, flags)) = keys.words(tag::NEEDS, j) {
                if needed == service && version != 0 && flags & 1 != 0 {
                    return Ok(Some((service, version)));
                }
                j += 1;
            }
            return Err(E_IMPORT);
        }
        i += 1;
    }
    Ok(None)
}

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
                if w.2 & 1 == 0 || discover::is_runtime_service(w.0, &services()) {
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

