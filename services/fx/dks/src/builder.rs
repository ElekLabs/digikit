//! The builder calls of the fx service as exports of the service file (`wrapper = "builder"` in sdk/sdk.toml): a module's `init` calls them, the loader links them to these entries,
//! and the state they fill is the owner's (`Builder`, reached through the file's cell). The calls that `patchkit gen` would write as thin wrappers cannot reach the owner, so these are
//! written by hand (`gen_exports.rs` checks their types). With no owner (before `init` made it or after `fini`) a call that opens a handle returns null, and the setters do nothing: a call outside the service's life
//! is as one outside `init`.
use dkrt_svc_rt::ImportCore;

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_module_ctx_add_fx(
    ctx: *mut u8,
    index: u32,
    name: *const u8,
    short_name: *const u8,
) -> *mut u8 {
    crate::DK_SVC_OWNER
        .with(|fx| {
            fx.builder()
                .add_fx(&ImportCore, ctx, index, name, short_name)
        })
        .unwrap_or(core::ptr::null_mut())
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_fx_add_page(
    fx: *mut u8,
    name: *const u8,
    short_name: *const u8,
) -> *mut u8 {
    crate::DK_SVC_OWNER
        .with(|owner| {
            owner
                .builder()
                .fx_add_page(&ImportCore, fx, name, short_name)
        })
        .unwrap_or(core::ptr::null_mut())
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_page_add_fx_param(
    p: *mut u8,
    name: *const u8,
    short_name: *const u8,
    kind: u32,
    min: f32,
    max: f32,
    def: f32,
) -> *mut u8 {
    crate::DK_SVC_OWNER
        .with(|owner| {
            owner
                .builder()
                .page_add_fx_param(&ImportCore, p, name, short_name, kind, min, max, def)
        })
        .unwrap_or(core::ptr::null_mut())
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_fx_routes(fx: *mut u8, allowed: u32, def: u32) {
    crate::DK_SVC_OWNER.with(|owner| owner.builder().fx_routes(&ImportCore, fx, allowed, def));
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_fx_mix(fx: *mut u8, def: u32) {
    crate::DK_SVC_OWNER.with(|owner| owner.builder().fx_mix(&ImportCore, fx, def));
}
