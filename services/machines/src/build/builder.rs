//! The builder calls (exported by the service file: `dks/src/builder.rs`).
use super::*;
use dkrt_abi::core_api::CoreApi;

// the builder calls (exported by the service file: `dks/src/builder.rs`); a call that names a handle comes from the module whose `init` is open

pub unsafe fn add_machine<C: CoreApi>(
    core: &C,
    ctx: *mut u8,
    index: u32,
    name: *const u8,
    short: *const u8,
) -> *mut u8 {
    if gate().open_for(core, ctx) {
        build().add_machine(index, name, short)
    } else {
        placeholder()
    }
}
pub unsafe fn machine_add_page<C: CoreApi>(
    core: &C,
    m: *mut u8,
    name: *const u8,
    short: *const u8,
) -> *mut u8 {
    if gate().open_any(core) {
        build().machine_add_page(m, name, short)
    } else {
        placeholder()
    }
}
pub unsafe fn page_add_param<C: CoreApi>(
    core: &C,
    p: *mut u8,
    name: *const u8,
    short: *const u8,
    def: u32,
) -> *mut u8 {
    if gate().open_any(core) {
        build().page_add_param(p, name, short, def)
    } else {
        placeholder()
    }
}
pub unsafe fn param_range<C: CoreApi>(core: &C, prm: *mut u8, min: u32, max: u32) {
    if gate().open_any(core) {
        build().param_range(prm, min, max);
    }
}
pub unsafe fn param_glyph<C: CoreApi>(core: &C, prm: *mut u8, glyph: u32) {
    if gate().open_any(core) {
        build().param_glyph(prm, glyph);
    }
}
pub unsafe fn param_latched<C: CoreApi>(core: &C, prm: *mut u8) {
    if gate().open_any(core) {
        build().param_latched(prm);
    }
}
pub unsafe fn param_on_click<C: CoreApi>(core: &C, prm: *mut u8, f: u32, arg: u32) {
    if gate().open_any(core) {
        build().param_on_click(prm, f, arg);
    }
}
pub unsafe fn page_canvas<C: CoreApi>(
    core: &C,
    p: *mut u8,
    flags: u32,
    draw: u32,
    event: u32,
    arg: u32,
) {
    if gate().open_any(core) {
        build().page_canvas(p, flags, draw, event, arg);
    }
}
pub unsafe fn page_bind<C: CoreApi>(core: &C, p: *mut u8, prm: *mut u8, slot: u32) {
    if gate().open_any(core) {
        build().page_bind(p, prm, slot);
    }
}
pub unsafe fn page_bind_local<C: CoreApi>(core: &C, p: *mut u8, local: u32, slot: u32) {
    if gate().open_any(core) {
        build().page_bind_local(p, local, slot);
    }
}
