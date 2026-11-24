//! The fixture of a service with a patch (RC4-0): the smallest provider that takes one `[[patch]]` of its manifest. `start` computes the bytes (the low word of a size bound that the firmware never reaches: `0xfffe`
//! for the stock `0xffff`, so the bytes of a code site change and nothing the firmware does); the installer writes them at the site of the port file when the service loads and writes the stock
//! bytes back before `stop`. It exports one function so that a module can need it.
#![no_std]

use dkrt_svc_rt::dkrt_abi::core_api::CoreApi;
use dkrt_svc_rt::dkrt_abi::provider::{Provider, Status};

/// The patch value: the low word of the immediate of `cmpi.l #0x3fffffff,D1` in the chooser's order vector copy (stock `0xffff`); D1 is always 6 there, so the bound is dead.
pub const BOUND_PATCH: u32 = 0xfffe;

/// The owner object: the value its `start` computed.
pub struct LocalPatch {
    bound: u32,
}

impl LocalPatch {
    /// The bytes of the patch, from the started instance.
    pub fn bound(&self) -> u32 {
        self.bound
    }
}

impl<C: CoreApi> Provider<C> for LocalPatch {
    fn start(_: &C) -> Result<LocalPatch, Status> {
        Ok(LocalPatch { bound: BOUND_PATCH })
    }
}

#[no_mangle]
pub extern "C" fn local_patch_value() -> u32 {
    7
}

dkrt_svc_rt::service!(
    LocalPatch,
    "local-patch",
    &[],
    parts = [],
    hooks = [],
    fills = [],
    patches = [("cf.filter.hook.test_bound_patch", 2, LocalPatch::bound)],
    exports = [("local_patch_value", local_patch_value)]
);
