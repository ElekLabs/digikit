//! `PackageHooks`: what the resident core calls on the system package (docs/plans/resident-core-and-system-package.md, section 2).
//!
//! One typed entry per crossing from the resident core into the package, grouped by the owner the resident code belongs to. An entry the package does not give
//! is `None`: the resident core keeps a defined default for it (`hooks.rs` of the runtime crate), so a core with no package still boots, serves the upload
//! commands it owns and writes the hang record. The entries are plain `extern "C"` function pointers with integers, pointers and `repr(C)` records, so a table
//! can come from another artifact. The resident core holds the installed table and gives each entry a typed caller; no resident file names the package.
//!
//! The table is not part of the service ABI (`SVC_ABI`): a service never sees it.

use dkrt_sdk::{DkrtCap, DkrtFitOut, DkrtFx, DkrtLive, DkrtModuleHeader, DkrtScan};

/// `CommandHooks::flags` answer for a command the package does not own.
pub const CMD_NONE: u32 = u32::MAX;
/// `DiagHooks::attribute` answer when no module is behind the site.
pub const NO_MODULE: u32 = u32::MAX;

/// The phases of `TickHooks::tick`: the three of the services (`service::tick_phase`: early, late, last) and the package's other work of the UI-loop pass. The
/// resident core runs its own steps between them, in the order of `dkrt_tick`.
pub mod tick_phase {
    /// The services' tick phases (`ServiceTable::tick`).
    pub use crate::service::tick_phase::{EARLY, LAST, LATE};
    /// The queued live job and the settings reload poll.
    pub const POLL_JOB: u32 = 3;
    /// The DSP link pump and the package manager's DSP step.
    pub const POLL_DSP: u32 = 4;
    /// The module files' jobs.
    pub const FILES: u32 = 5;
    /// The `tick` of every loaded module.
    pub const MODULES: u32 = 6;
}

/// What a module's slot adds to its listing entry (`dkrt_module`): the size of its DSP part and its context.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct ModuleExtra {
    pub dsp_size: u32,
    pub ctx: u32,
}

/// The DSP area as the health snapshot and the status report read it (`DiagHooks::dsp_area`).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct DspArea {
    pub total: u32,
    pub free: u32,
    pub largest: u32,
    /// `STATE_*` of the DSP loader.
    pub state: u32,
}

/// The boot (`boot::run` through `BootEnv`): the steps that reach the loader and the package manager.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BootHooks {
    /// A safe boot: the package loads nothing and the installer writes nothing.
    pub safe: Option<unsafe extern "C" fn()>,
    /// Read the file of each module of the selection record (a `sel::Record`) and keep its header table.
    pub read_tables: Option<unsafe extern "C" fn(rec: *const u8)>,
    /// Search the card for the providers the selection `count` modules long needs.
    pub find_providers: Option<unsafe extern "C" fn(count: u32)>,
    /// The package manager's load of the selection (a `sel::Record`).
    pub load: Option<unsafe extern "C" fn(rec: *const u8)>,
}

/// The UI-loop pass.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TickHooks {
    /// One `tick_phase`.
    pub tick: Option<unsafe extern "C" fn(phase: u32)>,
}

/// The upload commands the package owns (`transport.rs`): its built-in ones and those a module registered.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CommandHooks {
    /// The flags of a built-in command of the package (`NOLOG`, `BIG`, `STORE` of `transport.rs`), `CMD_NONE` for any other command.
    pub flags: Option<unsafe extern "C" fn(cmd: u32) -> u32>,
    /// Did a module register `cmd`?
    pub registered: Option<unsafe extern "C" fn(cmd: u32) -> bool>,
    /// Run the built-in command `cmd` of the package: the request number, the transport buffer, the body and its length; what its handler returns.
    pub run: Option<
        unsafe extern "C" fn(cmd: u32, seq: u32, buf: *mut u8, body: *const u8, blen: u32) -> i32,
    >,
    /// Run the command a module registered, with `out` the reply body: the length written (0 or more) or a negative status (`E_BUSY` while its module is reloaded,
    /// `E_PROTO` when no module registered it).
    pub run_registered: Option<
        unsafe extern "C" fn(cmd: u32, seq: u32, body: *const u8, blen: u32, out: *mut u8) -> i32,
    >,
}

/// What the resident exports to modules and the menu read of the loaded modules (`api.rs`, `watch.rs`, `report.rs`).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ModuleHooks {
    /// The record index of a module context: 0 or more; `E_CONTRACT` for a pointer that is not the live context of a content module.
    pub ctx_owner: Option<unsafe extern "C" fn(ctx: usize) -> i32>,
    /// `dkrt_load`: the module file `path` (NUL-terminated), 0 or a negative code.
    pub load_file: Option<unsafe extern "C" fn(path: *const u8) -> i32>,
    /// Read the DSP size and the context of the module with record index `i`.
    pub read_extra: Option<unsafe extern "C" fn(i: u32, out: *mut ModuleExtra)>,
    /// Removed (E1 step 4: the effects of a module are the fx service's `Describe` words in the MODULES reply): the word stays, always null; `dkrt_fx` answers `E_NOT_FOUND`.
    pub fx_entry: Option<unsafe extern "C" fn(index: u32, out: *mut DkrtFx) -> i32>,
    /// `dkrt_module_header`: read the header fields of the `.dkm` at store sector `first`.
    pub read_header: Option<unsafe extern "C" fn(first: u32, out: *mut DkrtModuleHeader) -> i32>,
    /// Does the module with record index `i` have a `tick`?
    pub has_tick: Option<unsafe extern "C" fn(i: u32) -> bool>,
    /// Removed (E1 step 4: `dk_debug_setup` takes any run-time type of the profile, and the host checks that a module has loaded it from the MODULES reply): the word stays, always null.
    pub machine_type_loaded: Option<unsafe extern "C" fn(ty: u32) -> bool>,
}

/// The exports the DIGIKIT menu calls (`dkrt_scan` and friends): the resident core keeps a symbol for each and calls through these.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MenuHooks {
    pub scan: Option<unsafe extern "C" fn(out: *mut DkrtScan, max: u32) -> i32>,
    pub scan_reads: Option<unsafe extern "C" fn() -> u32>,
    pub fit: Option<
        unsafe extern "C" fn(
            entries: *const DkrtScan,
            n: u32,
            ticked: *const u8,
            out: *mut DkrtFitOut,
        ) -> i32,
    >,
    pub capacity: Option<unsafe extern "C" fn(out: *mut DkrtCap, max: u32) -> u32>,
    pub live: Option<unsafe extern "C" fn(op: u32, path: *const u8) -> i32>,
    pub live_state: Option<unsafe extern "C" fn(out: *mut DkrtLive)>,
}

/// The diagnostics: the hang watch, the health snapshot and the DSP stats window.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct DiagHooks {
    /// Publish a full set of DSP diagnostics words (`DIAG_WORDS` of them) to the services.
    pub publish_words: Option<unsafe extern "C" fn(words: *const u32)>,
    /// The module behind the fault site `site` with call offset `off`, `NO_MODULE` for none.
    pub attribute: Option<unsafe extern "C" fn(site: u32, off: u32) -> u32>,
    /// The DSP area: total, free, largest block and the loader's state.
    pub dsp_area: Option<unsafe extern "C" fn(out: *mut DspArea)>,
    /// The owner of the span channel is gone: the DSP stops counting and the stats feed is unhooked.
    pub dsp_stats_owner_gone: Option<unsafe extern "C" fn()>,
}

/// The settings broker's reads of the package: which module is in its `init`, the settings reload and the overlay a service sets.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SettingsHooks {
    /// Is the module file with record index `owner` inside its `init`?
    pub in_init: Option<unsafe extern "C" fn(owner: u32) -> bool>,
    /// The module with record index `rec` opened its settings: a change of what it reads reloads it.
    pub opened: Option<unsafe extern "C" fn(rec: u32)>,
    /// The overlay's value of `key` for the module `id` into `buf`: the length, or a negative code; `NO_OVERLAY` when the overlay has no say.
    pub overlay_get: Option<
        unsafe extern "C" fn(
            id: *const u8,
            id_len: u32,
            key: *const u8,
            key_len: u32,
            buf: *mut u8,
            cap: u32,
        ) -> i32,
    >,
    /// The overlay takes a put: 0, or the refusal.
    pub overlay_put: Option<
        unsafe extern "C" fn(
            id: *const u8,
            id_len: u32,
            key: *const u8,
            key_len: u32,
            value: *const u8,
            len: u32,
        ) -> i32,
    >,
    /// The overlay takes a remove: 0, or the refusal.
    pub overlay_remove: Option<
        unsafe extern "C" fn(id: *const u8, id_len: u32, key: *const u8, key_len: u32) -> i32,
    >,
}

/// `overlay_get` answer when the overlay does not hold the key: the broker reads its own record.
pub const NO_OVERLAY: i32 = i32::MIN;

/// The provider facet queries of `dkrt_service_query` (`resident_query.rs`).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ProviderHooks {
    /// The address of the query function of `provider` at version `minimum` into `out`: 0, or a negative `E_*` code (then `out` is not written).
    pub query: Option<unsafe extern "C" fn(provider: u32, minimum: u32, out: *mut u32) -> i32>,
}

/// The first resident table minor whose package gives `PackageHooks::ui`. A package built against an older table has a shorter `PackageHooks`: the core does not read a
/// group the package predates (`hooks.rs` of the runtime crate), so the entry has its default there.
pub const UI_HOOKS_SINCE: u16 = 3;

/// The first resident table minor whose package has `UiHooks::record_name`: the `ui` group of an older package ends before it, so the core does not read the entry there.
pub const UI_RECORD_NAME_SINCE: u16 = 4;

/// What `core/ui` asks of the package: the rows the DIGIKIT menu adds to the SETTINGS rebuild, and the name of a record.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct UiHooks {
    /// Add the app's rows to the stock SETTINGS menu `menu` (the menu object the rebuild's hook gets). A null entry adds nothing: the stock menu stays as it is.
    pub settings_rows: Option<unsafe extern "C" fn(menu: *mut u8)>,
    /// The name of the loaded record `owner` (a module or a service file) into `out` (32 bytes, NUL padded): 0, or `E_NOT_FOUND` for a record that is not loaded. Appended with
    /// table minor `UI_RECORD_NAME_SINCE`.
    pub record_name: Option<unsafe extern "C" fn(owner: u32, out: *mut u8) -> i32>,
}

/// Every crossing from the resident core into the system package, by owner.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct PackageHooks {
    pub boot: BootHooks,
    pub tick: TickHooks,
    pub command: CommandHooks,
    pub module: ModuleHooks,
    pub menu: MenuHooks,
    pub diag: DiagHooks,
    pub settings: SettingsHooks,
    pub provider: ProviderHooks,
    /// Appended with table minor `UI_HOOKS_SINCE`.
    pub ui: UiHooks,
}
