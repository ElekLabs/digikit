use super::{Call, Probe};
use crate::install::{Entry, InstallError};
use crate::{PackageId, PackageInstaller};

pub struct FakeInstaller {
    probe: Probe,
}

impl FakeInstaller {
    pub fn new(probe: Probe) -> Self {
        FakeInstaller { probe }
    }
}

impl PackageInstaller for FakeInstaller {
    fn install(&mut self, owner: PackageId, _entries: &[Entry]) -> Result<(), InstallError> {
        self.probe
            .record(Call::Install(owner))
            .map_err(InstallError::Backend)
    }

    fn restore(&mut self, owner: PackageId) -> Result<(), InstallError> {
        let faulted = self.probe.record(Call::Restore(owner));
        if self.probe.take_busy(owner) {
            return Err(InstallError::Busy { task: None });
        }
        faulted.map_err(InstallError::Backend)
    }
}
