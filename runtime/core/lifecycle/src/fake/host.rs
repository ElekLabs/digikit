use super::{Call, Probe};
use crate::{PackageId, ProviderHost};
use dkrt_abi::provider::{Declarations, Limit, Status};
use std::vec::Vec;

/// Declares what it was told per package (nothing for a package it was not told about).
pub struct FakeHost {
    probe: Probe,
    declared: Vec<(PackageId, Vec<(u32, Limit)>)>,
}

impl FakeHost {
    pub fn new(probe: Probe) -> Self {
        FakeHost {
            probe,
            declared: Vec::new(),
        }
    }

    /// `package`'s `start` declares these resources (replacing an earlier answer).
    pub fn declares(&mut self, package: PackageId, resources: Vec<(u32, Limit)>) {
        self.declared.retain(|(p, _)| *p != package);
        self.declared.push((package, resources));
    }
}

impl ProviderHost for FakeHost {
    fn start(&mut self, package: PackageId) -> Result<Declarations, Status> {
        self.probe.record(Call::Start(package))?;
        let mut d = Declarations::new();
        if let Some((_, rs)) = self.declared.iter().find(|(p, _)| *p == package) {
            for &(name, limit) in rs {
                d.resource(name, limit)?;
            }
        }
        Ok(d)
    }

    fn activate(&mut self, package: PackageId) -> Result<(), Status> {
        self.probe.record(Call::Activate(package))
    }

    fn deactivate(&mut self, package: PackageId) {
        let _ = self.probe.record(Call::Deactivate(package));
    }

    fn attach(&mut self, service: PackageId, consumer: PackageId) -> Result<(), Status> {
        self.probe.record(Call::Attach(service, consumer))
    }

    fn detach(&mut self, service: PackageId, consumer: PackageId) {
        let _ = self.probe.record(Call::Detach(service, consumer));
    }

    fn can_stop(&mut self, package: PackageId) -> Result<(), Status> {
        self.probe.record(Call::CanStop(package))
    }

    fn stop(&mut self, package: PackageId) {
        let _ = self.probe.record(Call::Stop(package));
    }
}
