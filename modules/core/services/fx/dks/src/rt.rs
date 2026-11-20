//! The exports that only the runtime calls (`internal` imports of the `fx` service in sdk/sdk.toml, `wrapper = "rt"`): the runtime finds them by name when it needs
//! an answer that depends on a loaded service file (`handlers::exports`). A plain `extern "C"` function with no core argument: the file reaches the core through
//! `ImportCore`, as the builder exports do.
use dkrt_svc_fx::fxsend;
use dkrt_svc_rt::ImportCore;

/// The `index`th effect with a page into the `DkrtFx` at `out` (DIGIKIT > FX ROUTES): 0, or `E_NOT_FOUND` past the end.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_fx_routes_entry(index: u32, out: *mut u8) -> i32 {
    fxsend::fx_entry(&ImportCore, index, &mut *(out as *mut dkrt_sdk::DkrtFx))
}
