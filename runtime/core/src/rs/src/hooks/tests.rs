//! The defaults of the hooks with no package installed (a core-only level), and the completeness of the package's table.

use super::*;
use dkrt_sdk::fault::{DIAG_WORDS, SITE_NONE, SITE_VOICE};
use dkrt_sdk::proto::{CMD_MODULES, E_PROTO, E_STATE};
use dkrt_sdk::{E_BOOT_ONLY, E_CONTRACT, E_NOT_FOUND, OK};

fn locked() -> std::sync::MutexGuard<'static, ()> {
    crate::test_lock::LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner())
}

#[test]
fn the_package_table_has_every_hook() {
    let t = &crate::package::hooks::HOOKS;
    let missing = [
        ("boot.safe", t.boot.safe.is_none()),
        ("boot.read_tables", t.boot.read_tables.is_none()),
        ("boot.find_providers", t.boot.find_providers.is_none()),
        ("boot.load", t.boot.load.is_none()),
        ("tick.tick", t.tick.tick.is_none()),
                ("command.flags", t.command.flags.is_none()),
        ("command.registered", t.command.registered.is_none()),
        ("command.run", t.command.run.is_none()),
        ("command.run_registered", t.command.run_registered.is_none()),
        ("module.ctx_owner", t.module.ctx_owner.is_none()),
        ("module.load_file", t.module.load_file.is_none()),
        ("module.read_extra", t.module.read_extra.is_none()),
        ("module.fx_entry", t.module.fx_entry.is_none()),
        ("module.read_header", t.module.read_header.is_none()),
        ("module.has_tick", t.module.has_tick.is_none()),
        ("module.machine_type_loaded", t.module.machine_type_loaded.is_none()),
        ("menu.scan", t.menu.scan.is_none()),
        ("menu.scan_reads", t.menu.scan_reads.is_none()),
        ("menu.fit", t.menu.fit.is_none()),
        ("menu.capacity", t.menu.capacity.is_none()),
        ("menu.live", t.menu.live.is_none()),
        ("menu.live_state", t.menu.live_state.is_none()),
        ("diag.publish_words", t.diag.publish_words.is_none()),
        ("diag.attribute", t.diag.attribute.is_none()),
        ("diag.dsp_area", t.diag.dsp_area.is_none()),
        ("diag.dsp_stats_owner_gone", t.diag.dsp_stats_owner_gone.is_none()),
        ("settings.in_init", t.settings.in_init.is_none()),
        ("settings.opened", t.settings.opened.is_none()),
        ("settings.overlay_get", t.settings.overlay_get.is_none()),
        ("settings.overlay_put", t.settings.overlay_put.is_none()),
        ("settings.overlay_remove", t.settings.overlay_remove.is_none()),
        ("provider.query", t.provider.query.is_none()),
        ("ui.settings_rows", t.ui.settings_rows.is_none()),
    ];
    let gone: std::vec::Vec<&str> = missing.iter().filter(|(_, none)| *none).map(|(n, _)| *n).collect();
    assert!(gone.is_empty(), "hooks the package does not give: {gone:?}");
}

#[test]
fn with_no_package_every_hook_has_its_default() {
    let _g = locked();
    without_package(|| unsafe {
        assert!(!installed());
        // boot: nothing runs
        boot_safe();
        let rec = Record::empty();
        boot_read_tables(&rec);
        boot_find_providers(3);
        boot_load(&rec);
        // tick and mailbox
        tick(dkrt_abi::hooks::tick_phase::MODULES);
        assert_eq!(module_load(core::ptr::null()), E_BOOT_ONLY);
        // upload commands
        assert_eq!(command_flags(CMD_MODULES), None);
        assert!(!command_registered(0x40));
        let mut buf = [0u8; 8];
        assert_eq!(command_run(CMD_MODULES, 1, buf.as_mut_ptr(), buf.as_ptr(), 0), E_PROTO);
        assert_eq!(command_run_registered(0x40, 1, buf.as_ptr(), 0, buf.as_mut_ptr()), E_PROTO);
        // modules
        assert_eq!(ctx_owner(0x1000), E_CONTRACT);
        assert_eq!(module_extra(0), ModuleExtra::default());
        assert_eq!(fx_entry(0, core::ptr::null_mut()), E_NOT_FOUND);
        assert_eq!(module_header(0, core::ptr::null_mut()), E_NOT_FOUND);
        assert!(!module_has_tick(0));
        assert!(!machine_type_loaded(7));
        // the menu's exports
        assert_eq!(scan(core::ptr::null_mut(), 4), E_NOT_FOUND);
        assert_eq!(scan_reads(), 0);
        assert_eq!(fit(core::ptr::null(), 0, core::ptr::null(), core::ptr::null_mut()), E_NOT_FOUND);
        assert_eq!(capacity(core::ptr::null_mut(), 8), 0);
        assert_eq!(live(1, core::ptr::null()), E_STATE);
        let mut state = DkrtLive { phase: 9, result: 9, step: 9, unloaded: 9, op: 9 };
        live_state(&mut state);
        assert_eq!((state.phase, state.result, state.step, state.unloaded, state.op), (0, OK, 0, 0, 0));
        // diagnostics
        watch_diag(&[0; DIAG_WORDS]);
        assert_eq!(watch_attribute(1, 2), None);
        assert_eq!(dsp_area(), DspArea::default());
        dsp_stats_owner_gone();
        // settings
        assert!(!files_in_init(0));
        settings_opened(0);
        let mut out = [0u8; 4];
        assert_eq!(overlay_get(b"mod", b"key", &mut out), None);
        assert_eq!(overlay_put(b"mod", b"key", b"v"), OK);
        assert_eq!(overlay_remove(b"mod", b"key"), OK);
        // providers
        assert_eq!(provider_query(1, 1), Err(E_NOT_FOUND));
    });
}

/// `attach_package` is the one place the package gives its table: before it a core has none, after it the callers reach the package.
#[test]
fn attach_package_installs_the_package_table() {
    let _g = locked();
    without_package(|| unsafe {
        assert!(!installed());
        attach_package(&crate::resident::TABLE);
        assert!(installed());
        assert!(core::ptr::eq(table(), &crate::package::hooks::HOOKS));
    });
}

#[test]
fn with_the_package_the_hooks_reach_it() {
    let _g = locked();
    unsafe {
        assert!(installed());
        // a command of the package is known to the transport; a number nobody owns is not
        assert!(command_flags(CMD_MODULES).is_some());
        assert_eq!(command_flags(0x7e), None);
        // a pointer that is no module context
        assert_eq!(ctx_owner(0x1000), E_CONTRACT);
    }
}

/// The hang record with no package: the stop has no site when no call was pending, and no module is behind a site that was (the record is still written).
#[test]
fn the_hang_record_is_written_with_no_package() {
    let _g = locked();
    without_package(|| unsafe {
        let none = crate::watch::hang_site(0, 0, 0);
        assert_eq!((none.0, none.4), (SITE_NONE, None), "no call pending: no site");
        let voice = crate::watch::hang_site(0, (5 << 16) | 0x21, 0);
        assert_eq!((voice.0, voice.2, voice.4), (SITE_VOICE, 5, None), "a voice call was pending: its site and type, no module");
        let fx = crate::watch::hang_site((3 << 16) | (2 << 8) | 0x11, 0, 0);
        assert_eq!((fx.0, fx.1, fx.2, fx.4), (2, 0x11, 3, None), "an effect call was pending: its site, stage and offset, no module");
    });
}

#[test]
fn a_core_with_no_package_still_answers_through_its_exports_and_its_transport() {
    let _g = locked();
    without_package(|| unsafe {
        assert_eq!(crate::api::dkrt_ctx_owner(0x1000), E_CONTRACT);
        // a command of the package is unknown here (logged: it is no read-only built-in); the core's read-only ones stay unlogged
        assert!(crate::transport::logged(CMD_MODULES, 0));
        assert!(!crate::transport::logged(dkrt_sdk::proto::CMD_INFO, 0));
    });
}

// ---- ui.settings_rows ----------------------------------------------------------------------------------------------------

static RECORDED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

unsafe extern "C" fn record_rows(menu: *mut u8) {
    RECORDED.store(menu as usize, std::sync::atomic::Ordering::SeqCst);
}

static RECORDING: PackageHooks = PackageHooks {
    ui: dkrt_abi::hooks::UiHooks { settings_rows: Some(record_rows) },
    ..unsafe { core::mem::zeroed() }
};

/// Run `f` with `table` installed as built against `minor`, then give back the package of the binary (the lock is held).
fn with_table<R>(table: &'static PackageHooks, minor: u16, f: impl FnOnce() -> R) -> R {
    unsafe {
        install(table, minor);
        let r = f();
        install(&crate::package::hooks::HOOKS, dkrt_abi::resident::RESIDENT_MINOR);
        r
    }
}

/// The SETTINGS rows of the package are asked through the hook: the menu pointer arrives as it is. No package, a null entry and a package that predates the group each leave the stock
/// menu alone (no call, no fault).
#[test]
fn the_settings_rows_hook_reaches_the_package_and_has_a_null_default() {
    use dkrt_abi::hooks::UI_HOOKS_SINCE;
    use std::sync::atomic::Ordering::SeqCst;
    let _g = locked();
    unsafe {
        RECORDED.store(0, SeqCst);
        with_table(&RECORDING, UI_HOOKS_SINCE, || ui_settings_rows(0x1234 as *mut u8));
        assert_eq!(RECORDED.load(SeqCst), 0x1234, "the package's hook got the menu");

        RECORDED.store(0, SeqCst);
        with_table(&RECORDING, UI_HOOKS_SINCE - 1, || ui_settings_rows(0x1234 as *mut u8));
        assert_eq!(RECORDED.load(SeqCst), 0, "a package built before the group has no entry to read");

        static EMPTY: PackageHooks = unsafe { core::mem::zeroed() };
        with_table(&EMPTY, UI_HOOKS_SINCE, || ui_settings_rows(0x1234 as *mut u8));
        assert_eq!(RECORDED.load(SeqCst), 0, "a null entry adds nothing");

        without_package(|| ui_settings_rows(0x1234 as *mut u8));
        assert_eq!(RECORDED.load(SeqCst), 0, "no package: nothing");
    }
}
