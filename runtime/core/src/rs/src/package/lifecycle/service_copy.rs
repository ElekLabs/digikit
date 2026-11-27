//! A loaded copy of a service as the state handoff calls it (`Donor` of `CarriedState`): the row of its record and the runtime's core.
use super::carried_state::{Blocks, Donor};
use crate::package::loader::{Block, Images, Loader, Registry};
use dkrt_abi::dev_addr::DevAddr;
use dkrt_abi::package_platform::PackagePlatform;
use dkrt_abi::provider::ProviderHandle;
use dkrt_abi::MAX_MODULES;

/// The row of service record `rec` as it is loaded now.
pub(crate) struct ServiceCopy(ProviderHandle);

impl ServiceCopy {
    /// The loaded copy of record `rec`; none for a module record or a record with no row.
    pub(crate) fn of(rec: usize) -> Option<ServiceCopy> {
        if rec < MAX_MODULES {
            return None;
        }
        crate::package::provider_rows::row_of(rec).and_then(|r| r.first()).map(|h| ServiceCopy(*h))
    }

    /// `Provider::can_stop` of this copy.
    pub(crate) unsafe fn can_stop(&self) -> i32 {
        self.0.can_stop(self.core())
    }

    fn core(&self) -> *const core::ffi::c_void {
        &crate::package::core_api::REAL_CORE as *const _ as *const core::ffi::c_void
    }
}

impl Donor for ServiceCopy {
    fn name(&self) -> u32 {
        self.0.name()
    }
    fn versions(&self) -> Option<(u16, u16)> {
        self.0.state_versions()
    }
    unsafe fn export_max(&self) -> usize {
        self.0.state_export_max(self.core())
    }
    unsafe fn export(&self, out: &mut [u8]) -> usize {
        self.0.state_export(self.core(), out)
    }
    unsafe fn import(&self, from: Option<(u16, &[u8])>) -> i32 {
        self.0.state_import(self.core(), from)
    }
}

impl<P: PackagePlatform, M: Images, R: Registry> Blocks for Loader<P, M, R> {
    unsafe fn alloc(&mut self, n: u32) -> Option<Block> {
        self.arena_alloc(n)
    }
    unsafe fn free(&mut self, addr: DevAddr) {
        self.arena_free(addr)
    }
}
