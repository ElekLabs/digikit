//! The builder calls of the machines as the exports of the service file (`[[export]]` of module.toml, `provided = true` in sdk/sdk.toml): a module's `init` calls them, the
//! loader links them to these entries, and the state they fill is the service's (`dkrt_svc_machines::build`). Each is a thin entry over the crate's function and `ImportCore`.
use dkrt_sdk::api::{self, ParamClickFn};
use dkrt_svc_machines::build;
use dkrt_svc_rt::ImportCore;

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_module_ctx_add_machine(ctx: *mut u8, index: u32, name: *const u8, short_name: *const u8) -> *mut u8 {
    build::add_machine(&ImportCore, ctx, index, name, short_name)
}
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_machine_add_page(m: *mut u8, name: *const u8, short_name: *const u8) -> *mut u8 {
    build::machine_add_page(m, name, short_name)
}
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_page_add_param(p: *mut u8, name: *const u8, short_name: *const u8, def: u32) -> *mut u8 {
    build::page_add_param(p, name, short_name, def)
}
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_param_range(prm: *mut u8, min: u32, max: u32) {
    build::param_range(prm, min, max)
}
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_param_glyph(prm: *mut u8, glyph: u32) {
    build::param_glyph(prm, glyph)
}
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_param_latched(prm: *mut u8) {
    build::param_latched(prm)
}
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_param_on_click(prm: *mut u8, f: Option<ParamClickFn>, arg: *mut u8) {
    build::param_on_click(prm, f.map_or(0, |f| f as usize as u32), arg as usize as u32)
}
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_page_canvas(p: *mut u8, flags: u32, draw: Option<api::PageDrawFn>, event: Option<api::PageEventFn>, arg: *mut u8) {
    build::page_canvas(p, flags, draw.map_or(0, |f| f as usize as u32), event.map_or(0, |f| f as usize as u32), arg as usize as u32)
}
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_page_bind(p: *mut u8, prm: *mut u8, slot: u32) {
    build::page_bind(p, prm, slot)
}
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_page_bind_local(p: *mut u8, local: u32, slot: u32) {
    build::page_bind_local(p, local, slot)
}

// the entries have the types sdk/sdk.toml gives the imports
const _: api::DkModuleCtxAddMachineFn = dk_module_ctx_add_machine;
const _: api::DkMachineAddPageFn = dk_machine_add_page;
const _: api::DkPageAddParamFn = dk_page_add_param;
const _: api::DkParamRangeFn = dk_param_range;
const _: api::DkParamGlyphFn = dk_param_glyph;
const _: api::DkParamLatchedFn = dk_param_latched;
const _: api::DkParamOnClickFn = dk_param_on_click;
const _: api::DkPageCanvasFn = dk_page_canvas;
const _: api::DkPageBindFn = dk_page_bind;
const _: api::DkPageBindLocalFn = dk_page_bind_local;
