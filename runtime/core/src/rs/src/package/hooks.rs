//! The hooks the system package gives the resident core (`dkrt_abi::hooks::PackageHooks`): one function per crossing from the core into the package, and the table
//! of them. The core calls them through its own `hooks.rs` and names nothing of this folder; `install` gives it the table once, in `init_once`, and runs the package's
//! own first-use setup. A function with the right C signature is in the table as it is; the others have a thunk here.

use crate::hooks;
use crate::package::{commands, dsp, entry, files, handlers, link, lifecycle, loader, module_cmds, reload, scan, settings_overlay, settings_reload};
use dkrt_abi::hooks::*;
use dkrt_sdk::sel::Record;
use dkrt_sdk::proto::E_PROTO;
use dkrt_sdk::{DkrtFx, DkrtModuleHeader, E_BUSY, E_CONTRACT};

/// The package's table, in the one binary of the runtime.
pub(crate) static HOOKS: PackageHooks = PackageHooks {
    boot: BootHooks { safe: Some(boot_safe), read_tables: Some(boot_read_tables), find_providers: Some(boot_find_providers), load: Some(boot_load) },
    tick: TickHooks { tick: Some(tick) },
    command: CommandHooks { flags: Some(command_flags), registered: Some(command_registered), run: Some(command_run), run_registered: Some(command_run_registered) },
    module: ModuleHooks {
        ctx_owner: Some(ctx_owner),
        load_file: Some(load_file),
        read_extra: Some(module_extra),
        fx_entry: Some(fx_entry),
        read_header: Some(module_header),
        has_tick: Some(module_has_tick),
        machine_type_loaded: Some(machine_type_loaded),
    },
    menu: MenuHooks {
        scan: Some(scan::dkrt_scan),
        scan_reads: Some(scan::dkrt_scan_reads),
        fit: Some(scan::dkrt_fit),
        capacity: Some(crate::package::capacity::dkrt_capacity),
        live: Some(reload::dkrt_live),
        live_state: Some(reload::dkrt_live_state),
    },
    diag: DiagHooks { publish_words: Some(watch_diag), attribute: Some(watch_attribute), dsp_area: Some(dsp_area), dsp_stats_owner_gone: Some(dsp_stats_owner_gone) },
    settings: SettingsHooks {
        in_init: Some(files_in_init),
        opened: Some(settings_opened),
        overlay_get: Some(overlay_get),
        overlay_put: Some(overlay_put),
        overlay_remove: Some(overlay_remove),
    },
    provider: ProviderHooks { query: Some(provider_query) },
};

/// The package's first-use setup (`init_once`): the core gets the hooks, and the in-crate services declare their record kinds and the runtime compares them with the
/// generated table (declare-check-commit).
pub(crate) unsafe fn install() {
    hooks::install(&HOOKS);
    handlers::check_declarations();
}

// ---- boot ----------------------------------------------------------------------------------------------------------------

unsafe extern "C" fn boot_safe() {
    loader::set_safe()
}

unsafe extern "C" fn boot_read_tables(rec: *const u8) {
    crate::package::boot_steps::read_tables(&*(rec as *const Record))
}

unsafe extern "C" fn boot_find_providers(count: u32) {
    crate::package::boot_steps::find_providers(count as usize)
}

unsafe extern "C" fn boot_load(rec: *const u8) {
    crate::package::boot_steps::load(&*(rec as *const Record))
}

// ---- tick ----------------------------------------------------------------------------------------------------

/// One phase of the UI-loop pass. An if chain, not a `match`: a dense match becomes a PC-relative jump table, which is not ColdFire code.
unsafe extern "C" fn tick(phase: u32) {
    if phase == tick_phase::POLL_JOB {
        reload::poll();
        settings_reload::poll();
    } else if phase == tick_phase::POLL_DSP {
        link::pump();
        lifecycle::dsp_step();
    } else if phase == tick_phase::FILES {
        files::tick();
    } else if phase == tick_phase::MODULES {
        entry::tick_all();
    } else {
        handlers::services().tick(phase);
    }
}

// ---- upload commands -----------------------------------------------------------------------------------------------------

unsafe extern "C" fn command_flags(cmd: u32) -> u32 {
    match commands::find(cmd as u8) {
        Some(e) => e.flags as u32,
        None => CMD_NONE,
    }
}

unsafe extern "C" fn command_registered(cmd: u32) -> bool {
    module_cmds::service(cmd as u8).is_some()
}

unsafe extern "C" fn command_run(cmd: u32, seq: u32, buf: *mut u8, body: *const u8, blen: u32) -> i32 {
    match commands::find(cmd as u8) {
        Some(e) => (e.h)(seq, buf, body, blen as usize),
        None => E_PROTO,
    }
}

/// The command a module registered: counted as in flight for its module while the handler runs (`begin_cmd`, `end_cmd`), and what it logs carries its module's source
/// (owner 255: registered by no module).
unsafe extern "C" fn command_run_registered(cmd: u32, seq: u32, body: *const u8, blen: u32, out: *mut u8) -> i32 {
    match module_cmds::begin_cmd(cmd as u8) {
        Err(()) => E_BUSY, // its module is being reloaded: the host asks again
        Ok(Some((h, owner))) => {
            let prev = if owner != 255 && owner > 0 { crate::dlog::enter_module(owner as usize - 1, crate::dlog::PHASE_CALLBACK) } else { 0 };
            let r = h(cmd, seq, body, blen, out);
            if owner != 255 && owner > 0 {
                crate::dlog::leave_module(prev);
            }
            // counted since `begin_cmd`: a reload of the module waits for it
            module_cmds::end_cmd(owner);
            r
        }
        Ok(None) => E_PROTO,
    }
}

// ---- modules -------------------------------------------------------------------------------------------------------------

/// A service file's context is not given out: the unload observers are for content modules.
unsafe extern "C" fn ctx_owner(ctx: usize) -> i32 {
    match entry::ctx_owner(ctx as *mut u8) {
        Some(rec) if rec < dkrt_abi::MAX_MODULES => rec as i32,
        _ => E_CONTRACT,
    }
}

unsafe extern "C" fn load_file(path: *const u8) -> i32 {
    loader::load(path)
}

unsafe extern "C" fn module_extra(i: u32, out: *mut ModuleExtra) {
    let i = i as usize;
    *out = ModuleExtra { dsp_size: dsp::ds().mod_len[i], ctx: entry::ctx_addr(i).get() };
}

unsafe extern "C" fn fx_entry(index: u32, out: *mut DkrtFx) -> i32 {
    handlers::exports::fx_entry(index, &mut *out)
}

unsafe extern "C" fn module_header(first: u32, out: *mut DkrtModuleHeader) -> i32 {
    scan::module_header(first, out)
}

unsafe extern "C" fn module_has_tick(i: u32) -> bool {
    entry::has_tick(i as usize)
}

unsafe extern "C" fn machine_type_loaded(ty: u32) -> bool {
    module_cmds::machine_type_loaded(ty)
}

// ---- diagnostics ---------------------------------------------------------------------------------------------------------

unsafe extern "C" fn watch_diag(words: *const u32) {
    handlers::services().diag(&*(words as *const [u32; dkrt_sdk::fault::DIAG_WORDS]))
}

unsafe extern "C" fn watch_attribute(site: u32, off: u32) -> u32 {
    match handlers::services().attribute(site, off) {
        Some(m) => m as u32,
        None => NO_MODULE,
    }
}

unsafe extern "C" fn dsp_area(out: *mut DspArea) {
    let (free, largest) = dsp::area_stats();
    *out = DspArea { total: dsp::area_bytes(), free, largest, state: dsp::state() };
}

unsafe extern "C" fn dsp_stats_owner_gone() {
    crate::package::dsp_stats::owner_gone()
}

// ---- settings ------------------------------------------------------------------------------------------------------------

unsafe extern "C" fn files_in_init(owner: u32) -> bool {
    files::in_init(owner as usize)
}

unsafe extern "C" fn settings_opened(rec: u32) {
    settings_reload::note_opened(rec as usize)
}

unsafe fn text<'a>(p: *const u8, n: u32) -> &'a [u8] {
    if n == 0 {
        &[]
    } else {
        core::slice::from_raw_parts(p, n as usize)
    }
}

unsafe extern "C" fn overlay_get(id: *const u8, id_len: u32, key: *const u8, key_len: u32, buf: *mut u8, cap: u32) -> i32 {
    let mut none = [0u8; 0];
    let out: &mut [u8] = if cap == 0 { &mut none } else { core::slice::from_raw_parts_mut(buf, cap as usize) };
    settings_overlay::get(text(id, id_len), text(key, key_len), out).unwrap_or(NO_OVERLAY)
}

unsafe extern "C" fn overlay_put(id: *const u8, id_len: u32, key: *const u8, key_len: u32, value: *const u8, len: u32) -> i32 {
    settings_overlay::put(text(id, id_len), text(key, key_len), text(value, len))
}

unsafe extern "C" fn overlay_remove(id: *const u8, id_len: u32, key: *const u8, key_len: u32) -> i32 {
    settings_overlay::remove(text(id, id_len), text(key, key_len))
}

// ---- providers -----------------------------------------------------------------------------------------------------------

unsafe extern "C" fn provider_query(provider: u32, minimum: u32, out: *mut u32) -> i32 {
    match loader::resident_query(provider, minimum) {
        Ok(addr) => {
            *out = addr;
            dkrt_sdk::OK
        }
        Err(code) => code,
    }
}
