//! `ExportFns`: the exports the DIGIKIT menu in the package calls, and `core/ui`'s two calls it needs. The `dkrt_*` entries are the owners' own `extern "C"` functions
//! (the group's field types are the ones of `abi.toml`, so a changed signature fails here). `core/ui` is a module of the image that only this object links by name (`[object] optional` of the
//! module.toml: 0 when it is not in the image), so its two entries are thunks that check the address first.
use dkrt_abi::resident::{ExportFns, Group};

#[cfg(not(test))]
extern "C" {
    fn dk_ui_core_handle() -> *mut u8;
    fn dk_ui_stock_row(stock_menu: *mut u8, text: *const u8, run: *const (), arg: *mut u8);
}

/// Is the symbol linked (its address is not 0)? A host test links no firmware module, so there `core/ui` is absent and both thunks do nothing.
#[cfg(not(test))]
fn ui_linked(addr: usize) -> bool {
    core::hint::black_box(addr) != 0
}

unsafe extern "C" fn ui_core_handle() -> *mut u8 {
    #[cfg(not(test))]
    if ui_linked(dk_ui_core_handle as usize) {
        return dk_ui_core_handle();
    }
    core::ptr::null_mut()
}

unsafe extern "C" fn ui_stock_row(stock_menu: *mut u8, text: *const u8, run: *const (), arg: *mut u8) {
    #[cfg(not(test))]
    if ui_linked(dk_ui_stock_row as usize) {
        dk_ui_stock_row(stock_menu, text, run, arg);
    }
    #[cfg(test)]
    let _ = (stock_menu, text, run, arg);
}

pub(super) static FNS: Group<ExportFns> = Group::new(ExportFns {
    boot_info: crate::api::dkrt_boot_info,
    boot_name: crate::api::dkrt_boot_name,
    selection_get: crate::api::dkrt_selection_get,
    selection_set: crate::api::dkrt_selection_set,
    restart: crate::api::dkrt_restart,
    status: crate::report::dkrt_status,
    health: crate::health::dkrt_health,
    diag: crate::report::dkrt_diag,
    blocks: crate::health::dkrt_blocks,
    module: crate::api::dkrt_module,
    module_header: crate::api::dkrt_module_header,
    fx: crate::api::dkrt_fx,
    poll_add: crate::api::dkrt_poll_add,
    poll_remove: crate::api::dkrt_poll_remove,
    settings_put: crate::settings::dkrt_settings_put,
    settings_flush: crate::settings::dkrt_settings_flush,
    menu_built: crate::api::dkrt_menu_built,
    menu_release: crate::api::dkrt_menu_release,
    service_query: crate::resident_query::dkrt_service_query,
    ui_core_handle,
    ui_stock_row,
});
