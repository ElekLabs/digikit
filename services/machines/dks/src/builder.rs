//! The builder calls of the machines that take a callback, as exports of the service file (`wrapper = "builder"` in sdk/sdk.toml): a module's `init` calls them, the
//! loader links them to these entries, and the state they fill is the service's (`dkrt_svc_machines::build`). The other builder calls are thin wrappers that `patchkit gen`
//! writes to `gen_exports.rs`; these two convert the callback to an address first.
use dkrt_sdk::api::{self, ParamClickFn};
use dkrt_svc_machines::build;
use dkrt_svc_rt::ImportCore;

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_param_on_click(prm: *mut u8, f: Option<ParamClickFn>, arg: *mut u8) {
    build::param_on_click(&ImportCore, prm, f.map_or(0, |f| f as usize as u32), arg as usize as u32)
}
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_page_canvas(p: *mut u8, flags: u32, draw: Option<api::PageDrawFn>, event: Option<api::PageEventFn>, arg: *mut u8) {
    build::page_canvas(&ImportCore, p, flags, draw.map_or(0, |f| f as usize as u32), event.map_or(0, |f| f as usize as u32), arg as usize as u32)
}
