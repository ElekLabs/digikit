#![no_std]

use dkrt_svc_rt::dkrt_abi::{core_api::CoreApi, service::Service};

struct LocalPing;

impl<C: CoreApi> Service<C> for LocalPing {
    dkrt_svc_rt::dkrt_abi::accepts_records!();
    dkrt_svc_rt::dkrt_abi::accepts_teardown!();
}

unsafe fn setup(_: &dkrt_svc_rt::ImportCore) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn local_ping_value() -> u32 {
    42
}

dkrt_svc_rt::service!(
    LocalPing,
    "local-ping",
    &[],
    setup = setup,
    hooks = [],
    fills = [],
    exports = [("local_ping_value", local_ping_value)]
);
