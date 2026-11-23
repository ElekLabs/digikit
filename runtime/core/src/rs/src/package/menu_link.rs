//! The symbols of the DIGIKIT menu (C, `runtime/apps/digikit-menu`), which `patchkit syspkg build` links into the package artifact next to this crate. The menu calls
//! `dkrt_*` and `dk_ui_*` by name; the package links nothing by symbol, so each name is defined here and goes where the package can reach it:
//!
//! - the calls into the package itself (`dkrt_scan`, `dkrt_fit`, `dkrt_capacity`, `dkrt_live`, ...) are the package's own functions;
//! - the calls into the resident core (`dkrt_boot_info`, `dkrt_selection_get`, `dkrt_restart`, `dkrt_poll_add`, ...) go through the `exports` group of the resident table;
//! - the calls of `core/ui` (`dk_ui_core_handle`, `dk_ui_stock_row`) go through the same group; the other `dk_ui_*` calls are imports of a module, resolved by name the way a loaded module's
//!   are (`imports::import_addr`: the module's function, or its stub when `core/ui` is not in the image).
//!
//! Only in the `package-image` build: the resident object has its own `dkrt_*` symbols, and the host tests link no C.

use crate::package::resident::exports as table;
use crate::package::{capacity, imports, reload, scan};
use dkrt_abi::dev_addr::DevAddr;
use dkrt_sdk::api::{UiActionFn, UiChoiceFn, UiConfirmFn};
use dkrt_sdk::*;

/// `dkrt_*` of the package: the symbol is the package's function of the same name.
macro_rules! own {
    ($($name:ident = $path:path : fn($($a:ident: $t:ty),*) $(-> $r:ty)?;)*) => {$(
        #[no_mangle]
        pub unsafe extern "C" fn $name($($a: $t),*) $(-> $r)? {
            $path($($a),*)
        }
    )*};
}

/// `dkrt_*` of the resident core: the symbol calls the entry of the table facade.
macro_rules! resident {
    ($($name:ident = $f:ident : fn($($a:ident: $t:ty),*) $(-> $r:ty)?;)*) => {$(
        #[no_mangle]
        pub unsafe extern "C" fn $name($($a: $t),*) $(-> $r)? {
            table::$f($($a),*)
        }
    )*};
}

own! {
    dkrt_scan = scan::dkrt_scan: fn(out: *mut DkrtScan, max: u32) -> i32;
    dkrt_scan_reads = scan::dkrt_scan_reads: fn() -> u32;
    dkrt_fit = scan::dkrt_fit: fn(entries: *const DkrtScan, n: u32, ticked: *const u8, out: *mut DkrtFitOut) -> i32;
    dkrt_capacity = capacity::dkrt_capacity: fn(out: *mut DkrtCap, max: u32) -> u32;
    dkrt_live = reload::dkrt_live: fn(op: u32, path: *const u8) -> i32;
    dkrt_live_state = reload::dkrt_live_state: fn(out: *mut DkrtLive);
}

resident! {
    dkrt_boot_info = boot_info: fn(out: *mut DkrtBootInfo);
    dkrt_boot_name = boot_name: fn(index: u32, out: *mut u8) -> i32;
    dkrt_selection_get = selection_get: fn(out: *mut DkrtSelection) -> i32;
    dkrt_selection_set = selection_set: fn(sel: *const DkrtSelection) -> i32;
    dkrt_restart = restart: fn() -> i32;
    dkrt_status = status: fn(out: *mut DkrtStatus);
    dkrt_health = health: fn(out: *mut DkrtHealth);
    dkrt_diag = diag: fn(out: *mut DkrtDiag);
    dkrt_blocks = blocks: fn() -> u32;
    dkrt_module = module: fn(index: u32, out: *mut DkrtModule) -> i32;
    dkrt_module_header = module_header: fn(first: u32, out: *mut DkrtModuleHeader) -> i32;
    dkrt_fx = fx: fn(index: u32, out: *mut DkrtFx) -> i32;
    dkrt_poll_add = poll_add: fn(f: usize) -> i32;
    dkrt_poll_remove = poll_remove: fn(f: usize);
    dkrt_settings_put = settings_put: fn(key: *const u8, value: *const u8, len: u32) -> i32;
    dkrt_settings_flush = settings_flush: fn() -> i32;
    dkrt_menu_built = menu_built: fn(code: usize, data: usize);
    dkrt_menu_release = menu_release: fn(code: usize, data: usize);
    dkrt_service_query = service_query: fn(provider: u32, minimum: u32, op: u32, request: *const u8, request_size: u32, reply: *mut u8, reply_size: u32) -> i32;
    dk_ui_core_handle = ui_core_handle: fn() -> *mut u8;
    dk_ui_stock_row = ui_stock_row: fn(stock_menu: *mut u8, text: *const u8, run: *const (), arg: *mut u8);
}

/// The import `$name` of `core/ui` as a function of type `$ty`: its address through the import table, looked up at the call.
macro_rules! ui_import {
    ($($name:ident: fn($($a:ident: $t:ty),*) $(-> $r:ty)?;)*) => {$(
        #[no_mangle]
        pub unsafe extern "C" fn $name($($a: $t),*) $(-> $r)? {
            let addr = imports::import_addr(concat!(stringify!($name), "\0").as_ptr());
            let f: unsafe extern "C" fn($($t),*) $(-> $r)? = core::mem::transmute(DevAddr::new(addr).ptr::<()>());
            f($($a),*)
        }
    )*};
}

ui_import! {
    dk_ui_menu_new: fn(ui: *mut u8, desc: *const u8) -> *mut u8;
    dk_ui_confirm: fn(ui: *mut u8, title: *const u8, text: *const u8, done: Option<UiConfirmFn>, arg: *mut u8) -> i32;
    dk_ui_choice: fn(ui: *mut u8, title: *const u8, items: *const u8, n: i32, done: Option<UiChoiceFn>, arg: *mut u8) -> i32;
    dk_ui_menu_open: fn(m: *mut u8) -> i32;
    dk_ui_menu_close: fn(m: *mut u8);
    dk_ui_menu_select: fn(m: *mut u8, row: i32);
    dk_ui_menu_refresh: fn(m: *mut u8);
    dk_ui_menu_is_open: fn(m: *const u8) -> i32;
    dk_ui_menu_add_text: fn(m: *mut u8, text: *const u8);
    dk_ui_menu_add_action: fn(m: *mut u8, text: *const u8, run: Option<UiActionFn>, arg: *mut u8, value: i32);
    dk_ui_menu_add_submenu: fn(m: *mut u8, text: *const u8, sub: *mut u8);
    dk_ui_menu_check: fn(list: *mut u8, row: i32, on: i32);
}
