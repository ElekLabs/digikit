//! The resident state and the statics the package reads and writes through its views.
use super::table;
use dkrt_abi::runtime_state::PackageState;
use dkrt_abi::MAX_MODULES;

/// `DKRT_STATE`.
#[inline(always)]
pub(crate) fn st() -> &'static mut PackageState {
    unsafe { &mut *table().views.state }
}

/// `DKRT_PATHS`: the store path each module record was loaded from.
#[inline(always)]
pub(crate) fn paths() -> *mut [[u8; 64]; MAX_MODULES] {
    table().views.paths
}

/// `DKRT_MENUS`: the address of each record's `menu`.
#[inline(always)]
pub(crate) fn menus() -> *mut [u32; MAX_MODULES] {
    table().views.menus
}

/// `DKRT_MENU_OBJS`: whether a menu object was built for each record.
#[inline(always)]
pub(crate) fn menu_objs() -> *mut [u8; MAX_MODULES] {
    table().views.menu_objs
}

/// `DKRT_MODULE_RUNTIME`: the runtime version each record needs.
#[inline(always)]
pub(crate) fn module_runtime() -> *mut [u32; MAX_MODULES] {
    table().views.module_runtime
}

/// Was a menu object built for module record `rec` (`dkrt_menu_built`)?
#[inline(always)]
pub(crate) unsafe fn menus_held(rec: usize) -> bool {
    rec < MAX_MODULES && core::ptr::read_volatile(&raw const (*menu_objs())[rec]) != 0
}

/// First-use setup of the runtime: once per boot.
#[inline(always)]
pub(crate) unsafe fn init_once() {
    (table().state.init_once)()
}
