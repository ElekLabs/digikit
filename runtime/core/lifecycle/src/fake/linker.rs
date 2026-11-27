use super::{Call, Probe};
use crate::{Linker, PackageId};
use dkrt_abi::provider::Status;

pub struct FakeLinker {
    probe: Probe,
}

impl FakeLinker {
    pub fn new(probe: Probe) -> Self {
        FakeLinker { probe }
    }
}

impl Linker for FakeLinker {
    fn link(
        &mut self,
        package: PackageId,
        _at: u32,
        _services: &[PackageId],
    ) -> Result<(), Status> {
        self.probe.record(Call::Link(package))
    }

    fn unbind(&mut self, package: PackageId) {
        let _ = self.probe.record(Call::Unbind(package));
    }
}
