//! The resident core's calls into the system package. The installed `PackageHooks` table (`dkrt_abi::hooks`) is held here, and each hook has one typed caller: a
//! resident file reaches the package only through these functions. A hook the package did not install (or no package at all: a core-only level) has the default each
//! caller documents, so the core still boots, serves its own upload commands and writes the hang record.
//!
//! The one name of the package here is `attach_package`: the resident core gives the package its table and takes the package's hooks in return, once, in `init_once`; stage 1 will
//! do the same with a table from the package's own file.

use core::ptr::{addr_of, read_volatile, write_volatile};
use dkrt_abi::hooks::{DspArea, ModuleExtra, PackageHooks, CMD_NONE, NO_MODULE, NO_OVERLAY};
use dkrt_sdk::sel::Record;
use dkrt_sdk::proto::{E_PROTO, E_STATE};
use dkrt_sdk::{DkrtCap, DkrtFitOut, DkrtFx, DkrtLive, DkrtModuleHeader, DkrtScan, E_CONTRACT, E_NOT_FOUND, OK};

/// What a core with no package holds: no entry is given, so each caller answers with its documented default. `Option<fn>` is its null pointer when `None`, so the
/// all-zero table is the empty one.
static NO_PACKAGE: PackageHooks = unsafe { core::mem::zeroed() };

/// The table every typed caller reads: `NO_PACKAGE` until the package installs its own.
static mut INSTALLED: *const PackageHooks = &NO_PACKAGE;

/// The package gives its table: every typed caller below goes through it from now on. Once per boot; installing the same table again is harmless.
pub(crate) unsafe fn install(table: &'static PackageHooks) {
    write_volatile(core::ptr::addr_of_mut!(INSTALLED), table);
}

#[inline(always)]
fn table() -> &'static PackageHooks {
    unsafe { &*read_volatile(addr_of!(INSTALLED)) }
}

/// The package gets the resident table and gives its hooks: once per boot, in `init_once`.
pub(crate) unsafe fn attach_package(table: &'static dkrt_abi::resident::ResidentTable) {
    install(crate::package::attach(table));
}

/// Host tests: is a package table installed? (A core-only level has none.)
#[cfg(test)]
pub(crate) fn installed() -> bool {
    !core::ptr::eq(table(), &NO_PACKAGE)
}

/// Host tests: run `f` as a core with no package installed, then give back the table that was (the tests hold `test_lock::LOCK`).
#[cfg(test)]
pub(crate) fn without_package<R>(f: impl FnOnce() -> R) -> R {
    unsafe {
        let before = read_volatile(addr_of!(INSTALLED));
        write_volatile(core::ptr::addr_of_mut!(INSTALLED), &NO_PACKAGE);
        let r = f();
        write_volatile(core::ptr::addr_of_mut!(INSTALLED), before);
        r
    }
}

/// Call `table.<group>.<hook>(args)`; `default` when the hook is missing (always, with no package installed).
macro_rules! call {
    ($group:ident . $hook:ident ( $($arg:expr),* ), $default:expr) => {
        match table().$group.$hook {
            Some(f) => f($($arg),*),
            None => $default,
        }
    };
}

// ---- boot ----------------------------------------------------------------------------------------------------------------

/// A safe boot: the package loads nothing and the installer writes nothing. Default: nothing to do.
pub(crate) unsafe fn boot_safe() {
    call!(boot.safe(), ())
}

/// Read the file of each module of `rec` and keep its header table. Default: no tables.
pub(crate) unsafe fn boot_read_tables(rec: &Record) {
    call!(boot.read_tables(rec as *const Record as *const u8), ())
}

/// Search the card for the providers the selection `count` modules long needs. Default: none.
pub(crate) unsafe fn boot_find_providers(count: usize) {
    call!(boot.find_providers(count as u32), ())
}

/// The package manager's load of the selection. Default: nothing is loaded, every module stays as `boot::run` left it.
pub(crate) unsafe fn boot_load(rec: &Record) {
    call!(boot.load(rec as *const Record as *const u8), ())
}

// ---- tick ----------------------------------------------------------------------------------------------------

/// One phase of the UI-loop pass (`tick_phase`). Default: nothing runs.
pub(crate) unsafe fn tick(phase: u32) {
    call!(tick.tick(phase), ())
}

// ---- upload commands -----------------------------------------------------------------------------------------------------

/// The flags of a built-in command of the package; `None` for any other command (and for every command with no package).
pub(crate) unsafe fn command_flags(cmd: u8) -> Option<u8> {
    let f = call!(command.flags(cmd as u32), CMD_NONE);
    if f == CMD_NONE {
        None
    } else {
        Some(f as u8)
    }
}

/// Did a module register `cmd`? Default: no.
pub(crate) unsafe fn command_registered(cmd: u8) -> bool {
    call!(command.registered(cmd as u32), false)
}

/// Run the built-in command `cmd` of the package. Default: `E_PROTO`, a command the core does not know.
pub(crate) unsafe fn command_run(cmd: u8, seq: u32, buf: *mut u8, body: *const u8, blen: usize) -> i32 {
    call!(command.run(cmd as u32, seq, buf, body, blen as u32), E_PROTO)
}

/// Run the command a module registered; `out` takes the reply body. The length written, or a negative status. Default: `E_PROTO`.
pub(crate) unsafe fn command_run_registered(cmd: u8, seq: u32, body: *const u8, blen: usize, out: *mut u8) -> i32 {
    call!(command.run_registered(cmd as u32, seq, body, blen as u32, out), E_PROTO)
}

// ---- modules -------------------------------------------------------------------------------------------------------------

/// The record index of a module context, or `E_CONTRACT`. Default: `E_CONTRACT`: no module has a context.
pub(crate) unsafe fn ctx_owner(ctx: usize) -> i32 {
    call!(module.ctx_owner(ctx), E_CONTRACT)
}

/// `dkrt_load`: the module file `path`. Default: `E_BOOT_ONLY`, as for any caller after the boot.
pub(crate) unsafe fn module_load(path: *const u8) -> i32 {
    call!(module.load_file(path), dkrt_sdk::E_BOOT_ONLY)
}

/// The DSP size and the context of the module with record index `i`. Default: both 0.
pub(crate) unsafe fn module_extra(i: usize) -> ModuleExtra {
    let mut out = ModuleExtra::default();
    call!(module.read_extra(i as u32, &mut out), ());
    out
}

/// `dkrt_fx`. Default: `E_NOT_FOUND`: no effect has a page.
pub(crate) unsafe fn fx_entry(index: u32, out: *mut DkrtFx) -> i32 {
    call!(module.fx_entry(index, out), E_NOT_FOUND)
}

/// `dkrt_module_header`. Default: `E_NOT_FOUND`: no module file is read.
pub(crate) unsafe fn module_header(first: u32, out: *mut DkrtModuleHeader) -> i32 {
    call!(module.read_header(first, out), E_NOT_FOUND)
}

/// Does the module with record index `i` have a `tick`? Default: no.
pub(crate) unsafe fn module_has_tick(i: usize) -> bool {
    call!(module.has_tick(i as u32), false)
}

/// Does a loaded module have machine type `ty`? Default: no.
pub(crate) unsafe fn machine_type_loaded(ty: u32) -> bool {
    call!(module.machine_type_loaded(ty), false)
}

// ---- the menu's exports --------------------------------------------------------------------------------------------------

/// `dkrt_scan`. Default: `E_NOT_FOUND`, as for a card with no store.
pub(crate) unsafe fn scan(out: *mut DkrtScan, max: u32) -> i32 {
    call!(menu.scan(out, max), E_NOT_FOUND)
}

/// `dkrt_scan_reads`. Default: 0.
pub(crate) unsafe fn scan_reads() -> u32 {
    call!(menu.scan_reads(), 0)
}

/// `dkrt_fit`. Default: `E_NOT_FOUND`; `out` is not written.
pub(crate) unsafe fn fit(entries: *const DkrtScan, n: u32, ticked: *const u8, out: *mut DkrtFitOut) -> i32 {
    call!(menu.fit(entries, n, ticked, out), E_NOT_FOUND)
}

/// `dkrt_capacity`. Default: 0 entries.
pub(crate) unsafe fn capacity(out: *mut DkrtCap, max: u32) -> u32 {
    call!(menu.capacity(out, max), 0)
}

/// `dkrt_live`. Default: `E_STATE`: no job can be queued.
pub(crate) unsafe fn live(op: u32, path: *const u8) -> i32 {
    call!(menu.live(op, path), E_STATE)
}

/// `dkrt_live_state`. Default: an idle job (all words 0).
pub(crate) unsafe fn live_state(out: *mut DkrtLive) {
    call!(menu.live_state(out), *out = DkrtLive { phase: 0, result: OK, step: 0, unloaded: 0, op: 0 })
}

// ---- diagnostics ---------------------------------------------------------------------------------------------------------

/// A full set of DSP diagnostics words goes to the services. Default: nobody reads it.
pub(crate) unsafe fn watch_diag(words: &[u32; dkrt_sdk::fault::DIAG_WORDS]) {
    call!(diag.publish_words(words.as_ptr()), ())
}

/// The module behind the fault site `site` with call offset `off` (a record index). Default: none, so the hang record names no module.
pub(crate) unsafe fn watch_attribute(site: u32, off: u32) -> Option<usize> {
    let m = call!(diag.attribute(site, off), NO_MODULE);
    if m == NO_MODULE {
        None
    } else {
        Some(m as usize)
    }
}

/// The DSP area: total, free, largest block and the loader's state. Default: no area, an idle loader.
pub(crate) unsafe fn dsp_area() -> DspArea {
    let mut out = DspArea::default();
    call!(diag.dsp_area(&mut out), ());
    out
}

/// The owner of the span channel is gone. Default: no DSP stats run, nothing to stop.
pub(crate) unsafe fn dsp_stats_owner_gone() {
    call!(diag.dsp_stats_owner_gone(), ())
}

// ---- settings ------------------------------------------------------------------------------------------------------------

/// Is the module file with record index `owner` inside its `init`? Default: no.
pub(crate) unsafe fn files_in_init(owner: usize) -> bool {
    call!(settings.in_init(owner as u32), false)
}

/// The module `rec` opened its settings. Default: nothing reloads.
pub(crate) unsafe fn settings_opened(rec: usize) {
    call!(settings.opened(rec as u32), ())
}

/// The overlay's value of `key` for module `id`, or `None` when it has no say. Default: no overlay.
pub(crate) unsafe fn overlay_get(id: &[u8], key: &[u8], buf: &mut [u8]) -> Option<i32> {
    let r = call!(settings.overlay_get(id.as_ptr(), id.len() as u32, key.as_ptr(), key.len() as u32, buf.as_mut_ptr(), buf.len() as u32), NO_OVERLAY);
    if r == NO_OVERLAY {
        None
    } else {
        Some(r)
    }
}

/// The overlay takes a put: `OK` or its refusal. Default: `OK`.
pub(crate) unsafe fn overlay_put(id: &[u8], key: &[u8], value: &[u8]) -> i32 {
    call!(settings.overlay_put(id.as_ptr(), id.len() as u32, key.as_ptr(), key.len() as u32, value.as_ptr(), value.len() as u32), OK)
}

/// The overlay takes a remove: `OK` or its refusal. Default: `OK`.
pub(crate) unsafe fn overlay_remove(id: &[u8], key: &[u8]) -> i32 {
    call!(settings.overlay_remove(id.as_ptr(), id.len() as u32, key.as_ptr(), key.len() as u32), OK)
}

// ---- providers -----------------------------------------------------------------------------------------------------------

/// The address of the query function of `provider` at version `minimum`, or the negative code. Default: `E_NOT_FOUND`.
pub(crate) unsafe fn provider_query(provider: u32, minimum: u32) -> Result<u32, i32> {
    let mut addr = 0u32;
    let r = call!(provider.query(provider, minimum, &mut addr), E_NOT_FOUND);
    if r < 0 {
        Err(r)
    } else {
        Ok(addr)
    }
}

#[cfg(test)]
mod tests;
