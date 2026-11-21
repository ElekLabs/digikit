//! The exports that only resident image code calls (`internal` imports of the `fx` service in sdk/sdk.toml, `wrapper = "rt"`): the runtime finds them by name when it needs
//! an answer that depends on a loaded service file (`handlers::exports`). A plain `extern "C"` function with no core argument: the file reaches the core through
//! `ImportCore`, as the builder exports do.
use dkrt_svc_fx::fxsend;
use dkrt_svc_rt::ImportCore;

/// The `index`th effect with a page into the `DkrtFx` at `out` (DIGIKIT > FX ROUTES): 0, or `E_NOT_FOUND` past the end.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_fx_routes_entry(index: u32, out: *mut u8) -> i32 {
    fxsend::fx_entry(&ImportCore, index, &mut *(out as *mut dkrt_sdk::DkrtFx))
}

/// The FX resident query facet. Only the broker can enter the reserved export on device.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_service_query_v1(
    op: u32,
    request: *const u8,
    request_size: u32,
    reply: *mut u8,
    reply_size: u32,
) -> i32 {
    use core::mem::size_of;
    use dkrt_sdk::{E_CONTRACT, E_NOT_FOUND, OK};

    if op != fxsend::ROUTE_CHOICES_V1 {
        return E_NOT_FOUND;
    }
    if request.is_null()
        || reply.is_null()
        || request_size != size_of::<fxsend::RouteChoicesV1>() as u32
        || reply_size != size_of::<u32>() as u32
    {
        return E_CONTRACT;
    }
    let request = core::ptr::read_unaligned(request.cast::<fxsend::RouteChoicesV1>());
    match fxsend::route_choices(&ImportCore, request) {
        Ok(allowed) => {
            core::ptr::write_unaligned(reply.cast::<u32>(), allowed);
            OK
        }
        Err(code) => code,
    }
}
