use super::{Call, Probe};
use crate::{PackageId, Placer};
use dkrt_abi::provider::Status;

/// Places package `n` at `0x1000 * (n + 1)`.
pub struct FakePlacer {
    probe: Probe,
}

impl FakePlacer {
    pub fn new(probe: Probe) -> Self {
        FakePlacer { probe }
    }
}

impl Placer for FakePlacer {
    fn place(&mut self, package: PackageId) -> Result<u32, Status> {
        self.probe.record(Call::Place(package))?;
        Ok(0x1000 * (package.0 as u32 + 1))
    }

    fn free(&mut self, package: PackageId) {
        let _ = self.probe.record(Call::Free(package));
    }
}
