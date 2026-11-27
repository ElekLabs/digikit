//! The resident exports the DIGIKIT menu calls (`dkrt_abi::resident::ExportFns`): each under its own name, with the type of `abi.toml`. Used by `menu_link.rs` (the C menu's
//! symbols) only, so the menu reaches the resident core through the table and nothing else.
use super::table;
use dkrt_sdk::{DkrtBootInfo, DkrtDiag, DkrtFx, DkrtHealth, DkrtModule, DkrtModuleHeader, DkrtSelection, DkrtStatus};

#[inline(always)]
pub(crate) unsafe fn boot_info(out: *mut DkrtBootInfo) {
    (table().exports.boot_info)(out)
}

#[inline(always)]
pub(crate) unsafe fn boot_name(index: u32, out: *mut u8) -> i32 {
    (table().exports.boot_name)(index, out)
}

#[inline(always)]
pub(crate) unsafe fn selection_get(out: *mut DkrtSelection) -> i32 {
    (table().exports.selection_get)(out)
}

#[inline(always)]
pub(crate) unsafe fn selection_set(sel: *const DkrtSelection) -> i32 {
    (table().exports.selection_set)(sel)
}

#[inline(always)]
pub(crate) unsafe fn restart() -> i32 {
    (table().exports.restart)()
}

#[inline(always)]
pub(crate) unsafe fn status(out: *mut DkrtStatus) {
    (table().exports.status)(out)
}

#[inline(always)]
pub(crate) unsafe fn health(out: *mut DkrtHealth) {
    (table().exports.health)(out)
}

#[inline(always)]
pub(crate) unsafe fn diag(out: *mut DkrtDiag) {
    (table().exports.diag)(out)
}

#[inline(always)]
pub(crate) unsafe fn blocks() -> u32 {
    (table().exports.blocks)()
}

#[inline(always)]
pub(crate) unsafe fn module(index: u32, out: *mut DkrtModule) -> i32 {
    (table().exports.module)(index, out)
}

#[inline(always)]
pub(crate) unsafe fn module_header(first: u32, out: *mut DkrtModuleHeader) -> i32 {
    (table().exports.module_header)(first, out)
}

#[inline(always)]
pub(crate) unsafe fn fx(index: u32, out: *mut DkrtFx) -> i32 {
    (table().exports.fx)(index, out)
}

#[inline(always)]
pub(crate) unsafe fn poll_add(f: usize) -> i32 {
    (table().exports.poll_add)(f)
}

#[inline(always)]
pub(crate) unsafe fn poll_remove(f: usize) {
    (table().exports.poll_remove)(f)
}

#[inline(always)]
pub(crate) unsafe fn settings_put(key: *const u8, value: *const u8, len: u32) -> i32 {
    (table().exports.settings_put)(key, value, len)
}

#[inline(always)]
pub(crate) unsafe fn settings_flush() -> i32 {
    (table().exports.settings_flush)()
}

#[inline(always)]
pub(crate) unsafe fn menu_built(code: usize, data: usize) {
    (table().exports.menu_built)(code, data)
}

#[inline(always)]
pub(crate) unsafe fn menu_release(code: usize, data: usize) {
    (table().exports.menu_release)(code, data)
}

#[inline(always)]
pub(crate) unsafe fn service_query(provider: u32, minimum: u32, op: u32, request: *const u8, request_size: u32, reply: *mut u8, reply_size: u32) -> i32 {
    (table().exports.service_query)(provider, minimum, op, request, request_size, reply, reply_size)
}

/// `core/ui`'s `dk_ui_core_handle`: null when `core/ui` is not in the image.
#[inline(always)]
pub(crate) unsafe fn ui_core_handle() -> *mut u8 {
    (table().exports.ui_core_handle)()
}

/// `core/ui`'s `dk_ui_stock_row`: nothing when `core/ui` is not in the image.
#[inline(always)]
pub(crate) unsafe fn ui_stock_row(stock_menu: *mut u8, text: *const u8, run: *const (), arg: *mut u8) {
    (table().exports.ui_stock_row)(stock_menu, text, run, arg)
}
