//! The builder calls (exported by the service file: `dks/src/builder.rs`, which reaches the owner's `Builder`).
use super::*;
use dkrt_abi::core_api::CoreApi;

impl Builder {
    // the builder calls (exported by the service file: `dks/src/builder.rs`); a call that names a handle comes from the module whose `init` is open

    pub unsafe fn add_machine<C: CoreApi>(
        &mut self,
        core: &C,
        ctx: *mut u8,
        index: u32,
        name: *const u8,
        short: *const u8,
    ) -> *mut u8 {
        if self.build.gate.open_for(core, ctx) {
            self.build.add_machine(index, name, short)
        } else {
            self.build.placeholder()
        }
    }
    pub unsafe fn machine_add_page<C: CoreApi>(
        &mut self,
        core: &C,
        m: *mut u8,
        name: *const u8,
        short: *const u8,
    ) -> *mut u8 {
        if self.build.gate.open_any(core) {
            self.build.machine_add_page(m, name, short)
        } else {
            self.build.placeholder()
        }
    }
    pub unsafe fn page_add_param<C: CoreApi>(
        &mut self,
        core: &C,
        p: *mut u8,
        name: *const u8,
        short: *const u8,
        def: u32,
    ) -> *mut u8 {
        if self.build.gate.open_any(core) {
            self.build.page_add_param(p, name, short, def)
        } else {
            self.build.placeholder()
        }
    }
    pub unsafe fn param_range<C: CoreApi>(&mut self, core: &C, prm: *mut u8, min: u32, max: u32) {
        if self.build.gate.open_any(core) {
            self.build.param_range(prm, min, max);
        }
    }
    pub unsafe fn param_glyph<C: CoreApi>(&mut self, core: &C, prm: *mut u8, glyph: u32) {
        if self.build.gate.open_any(core) {
            self.build.param_glyph(prm, glyph);
        }
    }
    pub unsafe fn param_latched<C: CoreApi>(&mut self, core: &C, prm: *mut u8) {
        if self.build.gate.open_any(core) {
            self.build.param_latched(prm);
        }
    }
    pub unsafe fn param_on_click<C: CoreApi>(&mut self, core: &C, prm: *mut u8, f: u32, arg: u32) {
        if self.build.gate.open_any(core) {
            self.build.param_on_click(prm, f, arg);
        }
    }
    pub unsafe fn page_canvas<C: CoreApi>(
        &mut self,
        core: &C,
        p: *mut u8,
        flags: u32,
        draw: u32,
        event: u32,
        arg: u32,
    ) {
        if self.build.gate.open_any(core) {
            self.build.page_canvas(p, flags, draw, event, arg);
        }
    }
    pub unsafe fn page_bind<C: CoreApi>(&mut self, core: &C, p: *mut u8, prm: *mut u8, slot: u32) {
        if self.build.gate.open_any(core) {
            self.build.page_bind(p, prm, slot);
        }
    }
    pub unsafe fn page_bind_local<C: CoreApi>(
        &mut self,
        core: &C,
        p: *mut u8,
        local: u32,
        slot: u32,
    ) {
        if self.build.gate.open_any(core) {
            self.build.page_bind_local(p, local, slot);
        }
    }
}
