//! The builder calls of the machines as exports of the service file (`wrapper = "builder"` in sdk/sdk.toml): a module's `init` calls them, the loader links them to these entries,
//! and the state they fill is the owner's (`Builder`, reached through the file's cell). The calls that `patchkit gen` would write as thin wrappers cannot reach the owner, so these are
//! written by hand (`gen_exports.rs` checks their types); the two that take a callback convert it to an address first. With no owner (before `init` made it or after `fini`) a call
//! that opens a handle returns null, and the setters do nothing: a call outside the service's life is as one outside `init`.
use dkrt_sdk::api::{self, ParamClickFn};
use dkrt_svc_rt::ImportCore;

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_module_ctx_add_machine(
    ctx: *mut u8,
    index: u32,
    name: *const u8,
    short_name: *const u8,
) -> *mut u8 {
    crate::DK_SVC_OWNER
        .with(|m| {
            m.builder()
                .add_machine(&ImportCore, ctx, index, name, short_name)
        })
        .unwrap_or(core::ptr::null_mut())
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_machine_add_page(
    mach: *mut u8,
    name: *const u8,
    short_name: *const u8,
) -> *mut u8 {
    crate::DK_SVC_OWNER
        .with(|m| {
            m.builder()
                .machine_add_page(&ImportCore, mach, name, short_name)
        })
        .unwrap_or(core::ptr::null_mut())
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_page_add_param(
    p: *mut u8,
    name: *const u8,
    short_name: *const u8,
    def: u32,
) -> *mut u8 {
    crate::DK_SVC_OWNER
        .with(|m| {
            m.builder()
                .page_add_param(&ImportCore, p, name, short_name, def)
        })
        .unwrap_or(core::ptr::null_mut())
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_param_range(prm: *mut u8, min: u32, max: u32) {
    crate::DK_SVC_OWNER.with(|m| m.builder().param_range(&ImportCore, prm, min, max));
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_param_glyph(prm: *mut u8, glyph: u32) {
    crate::DK_SVC_OWNER.with(|m| m.builder().param_glyph(&ImportCore, prm, glyph));
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_param_latched(prm: *mut u8) {
    crate::DK_SVC_OWNER.with(|m| m.builder().param_latched(&ImportCore, prm));
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_param_on_click(prm: *mut u8, f: Option<ParamClickFn>, arg: *mut u8) {
    crate::DK_SVC_OWNER.with(|m| {
        m.builder().param_on_click(
            &ImportCore,
            prm,
            f.map_or(0, |f| f as usize as u32),
            arg as usize as u32,
        )
    });
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_page_canvas(
    p: *mut u8,
    flags: u32,
    draw: Option<api::PageDrawFn>,
    event: Option<api::PageEventFn>,
    arg: *mut u8,
) {
    crate::DK_SVC_OWNER.with(|m| {
        m.builder().page_canvas(
            &ImportCore,
            p,
            flags,
            draw.map_or(0, |f| f as usize as u32),
            event.map_or(0, |f| f as usize as u32),
            arg as usize as u32,
        )
    });
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_page_bind(p: *mut u8, prm: *mut u8, slot: u32) {
    crate::DK_SVC_OWNER.with(|m| m.builder().page_bind(&ImportCore, p, prm, slot));
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_page_bind_local(p: *mut u8, local: u32, slot: u32) {
    crate::DK_SVC_OWNER.with(|m| m.builder().page_bind_local(&ImportCore, p, local, slot));
}
