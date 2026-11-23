//! `setup` of the file.
use super::scope::SCOPE;
use super::sdk_imports as sys;
use super::service_state::{FILES, UI};
use core::ptr::addr_of_mut;
use dkrt_abi::core_api::CoreApi;
use dkrt_svc_rt::ImportCore;

/// `setup` of the file (the first step of `init`): the listing client that this file's `init` is the only place to open, and the project scope table the runtime's settings overlay forwards to.
pub unsafe fn setup(core: &ImportCore) -> i32 {
    let ctx = dkrt_svc_rt::init_ctx();
    let mut f: *mut u8 = core::ptr::null_mut();
    let r = sys::dk_files_open(ctx, &mut f);
    if r != 0 {
        return r;
    }
    *addr_of_mut!(FILES) = f;
    // optional: without core/ui the result is a failure and the handle stays null
    let mut ui: *mut u8 = core::ptr::null_mut();
    if sys::dk_ui_open(ctx, &mut ui) == 0 {
        *addr_of_mut!(UI) = ui;
    }
    core.settings_overlay_set(&SCOPE);
    0
}
