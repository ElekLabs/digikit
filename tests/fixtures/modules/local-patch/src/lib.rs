//! The fixture of a service with a patch (RC4-0): the smallest provider that takes one `[[patch]]` of its manifest. `start` computes the bytes (a two-byte allocation size above the
//! stock `0x18`, so the firmware allocates more than it copies and nothing else changes); the installer writes them at the site of the port file when the service loads and writes the stock
//! bytes back before `stop`. It exports one function so that a module can need it.
#![no_std]

use dkrt_svc_rt::dkrt_abi::core_api::CoreApi;
use dkrt_svc_rt::dkrt_abi::provider::{Provider, Status};

/// The patch value: the allocation size of the chooser's order vector, above the stock `0x18`.
pub const LIST_SIZE_PATCH: u32 = 0x20;

/// The owner object: the value its `start` computed.
pub struct LocalPatch {
    list_size: u32,
}

impl LocalPatch {
    /// The bytes of the patch, from the started instance.
    pub fn list_size(&self) -> u32 {
        self.list_size
    }
}

impl<C: CoreApi> Provider<C> for LocalPatch {
    fn start(_: &C) -> Result<LocalPatch, Status> {
        Ok(LocalPatch { list_size: LIST_SIZE_PATCH })
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
    patches = [("cf.filter.hook.test_list_size_patch", 2, LocalPatch::list_size)],
    exports = [("local_patch_value", local_patch_value)]
);
