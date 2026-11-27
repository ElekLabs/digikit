use super::{Call, Probe};
use crate::{LiveSet, PackageGraph, PackageId, ProviderHost};
use dkrt_abi::provider::{Declarations, Limit, Status};
use std::vec::Vec;

/// Declares what it was told per package (nothing for a package it was not told about).
pub struct FakeHost {
    probe: Probe,
    declared: Vec<(PackageId, Vec<(u32, Limit)>)>,
    dsp: Vec<PackageId>,
    ranks: Vec<(PackageId, u32)>,
}

impl FakeHost {
    pub fn new(probe: Probe) -> Self {
        FakeHost {
            probe,
            declared: Vec::new(),
            dsp: Vec::new(),
            ranks: Vec::new(),
        }
    }

    /// `package` has a DSP part: `start_dsp` answers `true` for it.
    pub fn has_dsp(&mut self, package: PackageId) {
        self.dsp.push(package);
    }

    /// `service`'s callbacks run at `rank`.
    pub fn ranks(&mut self, service: PackageId, rank: u32) {
        self.ranks.push((service, rank));
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

    fn services_up(&mut self, _graph: &PackageGraph, _live: &LiveSet) {
        let _ = self.probe.record(Call::ServicesUp);
    }

    fn attach_rank(&self, service: PackageId) -> u32 {
        match self.ranks.iter().find(|(p, _)| *p == service) {
            Some(&(_, r)) => r,
            None => service.0 as u32,
        }
    }

    fn attach(&mut self, consumer: PackageId, services: &[PackageId]) -> Result<(), Status> {
        self.probe.walk(consumer, services);
        self.probe.record(Call::Attach(consumer))
    }

    fn detach(&mut self, consumer: PackageId, services: &[PackageId]) {
        self.probe.walk(consumer, services);
        let _ = self.probe.record(Call::Detach(consumer));
    }

    fn can_stop(&mut self, package: PackageId) -> Result<(), Status> {
        self.probe.record(Call::CanStop(package))
    }

    fn export_state(&mut self, package: PackageId) {
        let _ = self.probe.record(Call::ExportState(package));
    }

    fn import_state(&mut self, package: PackageId) -> Result<(), Status> {
        self.probe.record(Call::ImportState(package))
    }

    fn stop(&mut self, package: PackageId) {
        let _ = self.probe.record(Call::Stop(package));
    }

    fn start_dsp(&mut self, package: PackageId) -> bool {
        let has = self.dsp.contains(&package);
        if has {
            let _ = self.probe.record(Call::StartDsp(package));
        }
        has
    }
}
