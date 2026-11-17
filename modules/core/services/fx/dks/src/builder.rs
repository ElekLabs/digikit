//! The builder calls of the effects as the exports of the service file (`[[export]]` of module.toml, `provided = true` in sdk/sdk.toml): a module's `init` calls them, the
//! loader links them to these entries, and the state they fill is the service's (`dkrt_svc_fx::build`). Each is a thin entry over the crate's function and `ImportCore`.
use dkrt_sdk::api;
use dkrt_svc_fx::build;
use dkrt_svc_rt::ImportCore;

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_module_ctx_add_fx(ctx: *mut u8, index: u32, name: *const u8, short_name: *const u8) -> *mut u8 {
    build::add_fx(&ImportCore, ctx, index, name, short_name)
}
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_fx_add_page(fx: *mut u8, name: *const u8, short_name: *const u8) -> *mut u8 {
    build::fx_add_page(fx, name, short_name)
}
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_page_add_fx_param(p: *mut u8, name: *const u8, short_name: *const u8, kind: u32, min: f32, max: f32, def: f32) -> *mut u8 {
    build::page_add_fx_param(p, name, short_name, kind, min, max, def)
}
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_fx_routes(fx: *mut u8, allowed: u32, def: u32) {
    build::fx_routes(fx, allowed, def)
}
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_fx_mix(fx: *mut u8, def: u32) {
    build::fx_mix(fx, def)
}

// the entries have the types sdk/sdk.toml gives the imports
const _: api::DkModuleCtxAddFxFn = dk_module_ctx_add_fx;
const _: api::DkFxAddPageFn = dk_fx_add_page;
const _: api::DkPageAddFxParamFn = dk_page_add_fx_param;
const _: api::DkFxRoutesFn = dk_fx_routes;
const _: api::DkFxMixFn = dk_fx_mix;
