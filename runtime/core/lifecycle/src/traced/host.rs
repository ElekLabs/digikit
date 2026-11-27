use crate::{LiveSet, PackageGraph, PackageId, Phase, ProviderHost, Step, Trace, Traced};
use dkrt_abi::provider::{Declarations, Status};

fn of(r: &Result<(), Status>) -> i32 {
    r.err().unwrap_or(0)
}

impl<S: ProviderHost, T: Trace> ProviderHost for Traced<S, T> {
    fn start(&mut self, package: PackageId) -> Result<Declarations, Status> {
        self.run(
            Step::Start,
            package,
            |s| s.start(package),
            |r| r.as_ref().err().copied().unwrap_or(0),
        )
    }

    fn activate(&mut self, package: PackageId) -> Result<(), Status> {
        self.run(Step::Activate, package, |s| s.activate(package), of)
    }

    fn deactivate(&mut self, package: PackageId) {
        self.run(Step::Deactivate, package, |s| s.deactivate(package), |_| 0)
    }

    fn attach_rank(&self, service: PackageId) -> u32 {
        self.inner.attach_rank(service)
    }

    fn attach(&mut self, consumer: PackageId, services: &[PackageId]) -> Result<(), Status> {
        self.run(Step::Attach, consumer, |s| s.attach(consumer, services), of)
    }

    fn detach(&mut self, consumer: PackageId, services: &[PackageId]) {
        self.run(
            Step::Detach,
            consumer,
            |s| s.detach(consumer, services),
            |_| 0,
        )
    }

    fn can_stop(&mut self, package: PackageId) -> Result<(), Status> {
        self.run(Step::CanStop, package, |s| s.can_stop(package), of)
    }

    fn export_state(&mut self, package: PackageId) {
        self.run(
            Step::ExportState,
            package,
            |s| s.export_state(package),
            |_| 0,
        )
    }

    fn import_state(&mut self, package: PackageId) -> Result<(), Status> {
        self.run(Step::ImportState, package, |s| s.import_state(package), of)
    }

    fn stop(&mut self, package: PackageId) {
        self.run(Step::Stop, package, |s| s.stop(package), |_| 0)
    }

    fn services_up(&mut self, graph: &PackageGraph, live: &LiveSet) {
        self.inner.services_up(graph, live)
    }

    fn start_dsp(&mut self, package: PackageId) -> bool {
        // most packages have no DSP part: only the start of one is an event (a note: the part comes up over later passes, and the host reports its end)
        let coming = self.inner.start_dsp(package);
        if coming && self.trace.on() {
            let name = self.trace.name_of(package);
            self.trace
                .event(Phase::Note, Step::DspLoad, package, name, 0);
        }
        coming
    }
}
