//! The `.dkm` loader (docs/design/digikit-dkm.md). Every check runs before anything is written to the arena.

use crate::package::dsp;
use dkrt_abi::MAX_MODULES;
use dkrt_abi::mem::cstr_eq;
use dkrt_abi::mem::cstr_len;
use dkrt_abi::mem::rd32;
use dkrt_abi::mem::wr32;
use dkrt_abi::module::Ctx;
use crate::package::admission::Admission;
use discover::Found;
use dkrt_abi::plan::Verdict;
use crate::package::handlers::{services, ServiceTable};
use dkneeds::keys::Keys;
use install::{Backend, CfItems, DspInstaller, Installer, SITES_MAX};
use role::{PlaceArgs, RoleSteps};
use exports::Exports;
use crate::platform::RealPlatform;
use crate::selection::BOOT_IDX;
use crate::platform::st;
use dkrt_abi::module::{Module, LOG_NAME, PROVIDER_IDX};
use core::ptr::{addr_of, addr_of_mut, read_volatile};
use dkrt_abi::dev_addr::DevAddr;
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

mod check;
mod commit;
pub(crate) mod discover;
pub(crate) mod exports;
pub(crate) mod install;
mod live;
mod load;
pub(crate) mod package;
pub(crate) use package::PhaseKind;
mod plan;
mod port;
mod quiesce;
pub(crate) mod records;
pub(crate) mod role;
mod seams;
mod unload;
#[cfg(test)]
pub(crate) mod tests;
use crate::image::{self, in_file};
use check::{Checked, Placed};
use plan::{parse_provs, service_import_owner, table_of_file, table_span};
#[cfg(test)]
pub(crate) use plan::table_keys;
#[cfg(test)]
use plan::{settle_ctx, unmet_needs, WANT_MAX};
#[cfg(test)]
pub(crate) use live::{live_caps, own_use};
pub use live::{live_check_load, reload_check, retire_index};
pub use quiesce::{reload_abort, reload_quiesce};
pub(crate) use live::{live_capacity, live_indices};
pub use plan::{capture, find_providers, keys_of, planned_dsp, reset_plan};
pub(crate) use plan::{boot_caps, live_settle_ctx};
#[cfg(test)]
pub(crate) use plan::test_accept;
pub(crate) use unload::retire_sdk;
pub use seams::{Block, Images, RealImages, RealRegistry, Registry, Staged};
pub(crate) use seams::{DeviceImages, DEVICE_IMAGES};

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
pub(crate) type Real = Loader<RealPlatform, DeviceImages, RealRegistry>;

static mut LOADER: Real = Loader::new(RealPlatform, DEVICE_IMAGES, RealRegistry);

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
    loader().set_safe()
}

/// The capacity of the platform: `dkrt_capacity` into `caps`; its count.
pub(crate) fn dkrt_capacity_into(caps: &mut [DkrtCap; CAP_MAX]) -> usize {
    unsafe { crate::package::capacity::dkrt_capacity(caps.as_mut_ptr(), CAP_MAX as u32) as usize }
}

/// The module record loaded from the store path `path` (NUL terminated), and its selection index: what a live job names its module by.
pub unsafe fn rec_of_path(path: *const u8) -> Option<(usize, usize)> {
    loader().rec_of_path(path)
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

/// The name of the missing module of the last load that failed with `E_IMPORT` (`Loader::missing`).
pub(crate) fn missing() -> [u8; NEEDS_LEN] {
    loader().missing()
}

/// A host test seeds a record by hand: no teardown of an earlier test is half done.
#[cfg(test)]
pub(crate) unsafe fn test_clear_torn() {
    loader().torn = [0; records::REC_MAX];
}

impl<P: Platform, M: Images, R: Registry> Loader<P, M, R> {
    /// Safe mode: the installer writes nothing.
    pub(crate) fn set_safe(&mut self) {
        self.installer.set_armed(false)
    }

    /// The module record loaded from the store path `path` (NUL terminated), and its selection index: what a live job names its module by.
    pub(crate) unsafe fn rec_of_path(&self, path: *const u8) -> Option<(usize, usize)> {
        let mut rec = 0;
        while rec < MAX_MODULES {
            if records::module(rec).used != 0 && cstr_eq(records::path(rec).as_ptr(), path) {
                let k = self.sel_of(rec) as usize;
                return if k < SEL_MAX { Some((rec, k)) } else { None };
            }
            rec += 1;
        }
        None
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

    /// The name of the missing module of the last load that failed with `E_IMPORT`, NUL padded; empty when none.
    pub fn missing(&self) -> [u8; NEEDS_LEN] {
        self.missing
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
