//! `ExportFns`: the `dkrt_*` exports of the resident core that the DIGIKIT menu in the package calls, and the two calls of `core/ui` it needs. The menu is C code linked into the
//! package; its calls reach the resident core through these entries and nothing else. Each function type is the one `abi.toml` gives the export (`dkrt_sdk::sig`), so a
//! changed signature fails the build in the resident crate (the group is filled with the exports themselves).

use dkrt_sdk::sig;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ExportFns {
    pub boot_info: sig::dkrt_boot_info,
    pub boot_name: sig::dkrt_boot_name,
    pub selection_get: sig::dkrt_selection_get,
    pub selection_set: sig::dkrt_selection_set,
    pub restart: sig::dkrt_restart,
    pub status: sig::dkrt_status,
    pub health: sig::dkrt_health,
    pub diag: sig::dkrt_diag,
    pub blocks: sig::dkrt_blocks,
    pub module: sig::dkrt_module,
    pub module_header: sig::dkrt_module_header,
    pub fx: sig::dkrt_fx,
    pub poll_add: sig::dkrt_poll_add,
    pub poll_remove: sig::dkrt_poll_remove,
    pub settings_put: sig::dkrt_settings_put,
    pub settings_flush: sig::dkrt_settings_flush,
    pub menu_built: sig::dkrt_menu_built,
    pub menu_release: sig::dkrt_menu_release,
    pub service_query: sig::dkrt_service_query,
    /// `core/ui`'s `dk_ui_core_handle`: the UI handle of a core module; null when `core/ui` is not in the image.
    pub ui_core_handle: unsafe extern "C" fn() -> *mut u8,
    /// `core/ui`'s `dk_ui_stock_row(stock_menu, text, fn, arg)`: a row among the rows of a stock menu; nothing when `core/ui` is not in the image.
    pub ui_stock_row:
        unsafe extern "C" fn(stock_menu: *mut u8, text: *const u8, run: *const (), arg: *mut u8),
}
