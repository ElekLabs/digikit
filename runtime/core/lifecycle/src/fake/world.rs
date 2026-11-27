use super::{
    FakeHost, FakeInstaller, FakeLinker, FakeManifest, FakePlacer, Pkg, Probe, RecordingTrace,
};
use crate::{PackageManager, Traced};
use dkrt_abi::provider::Limit;
use std::vec::Vec;

/// A package manager over the fakes.
pub type FakeManager =
    PackageManager<FakeManifest, FakePlacer, FakeLinker, FakeInstaller, FakeHost>;

/// A package manager over the fakes with every seam traced to a [`RecordingTrace`].
pub type TracedManager = PackageManager<
    FakeManifest,
    Traced<FakePlacer, RecordingTrace>,
    Traced<FakeLinker, RecordingTrace>,
    Traced<FakeInstaller, RecordingTrace>,
    Traced<FakeHost, RecordingTrace>,
>;

/// The fake host of `pkgs`: each service declares the pools of its [`Pkg`].
fn host_of(pkgs: &[Pkg], probe: &Probe) -> FakeHost {
    let mut host = FakeHost::new(probe.clone());
    for p in pkgs {
        let declared: Vec<_> = p
            .resources
            .iter()
            .map(|r| (r.name(), Limit::Fixed(r.limit())))
            .collect();
        host.declares(p.id, declared);
        if let Some(r) = p.rank {
            host.ranks(p.id, r);
        }
        if p.dsp {
            host.has_dsp(p.id);
        }
    }
    host
}

/// A manager for `pkgs`, nothing up, and the probe all its seams record into. Each service declares the pools of its [`Pkg`].
pub fn manager(pkgs: &[Pkg]) -> (FakeManager, Probe) {
    let probe = Probe::default();
    let pm = PackageManager::new(
        FakeManifest::new(pkgs).with_probe(probe.clone()),
        FakePlacer::new(probe.clone()),
        FakeLinker::new(probe.clone()),
        FakeInstaller::new(probe.clone()),
        host_of(pkgs, &probe),
    )
    .expect("the fake packages form a graph");
    (pm, probe)
}

/// [`manager`] with every seam traced: the probe, and the trace the wrappers report to.
pub fn traced_manager(pkgs: &[Pkg]) -> (TracedManager, Probe, RecordingTrace) {
    let probe = Probe::default();
    let trace = RecordingTrace::default();
    let pm = PackageManager::new(
        FakeManifest::new(pkgs).with_probe(probe.clone()),
        Traced::new(FakePlacer::new(probe.clone()), trace.clone()),
        Traced::new(FakeLinker::new(probe.clone()), trace.clone()),
        Traced::new(FakeInstaller::new(probe.clone()), trace.clone()),
        Traced::new(host_of(pkgs, &probe), trace.clone()),
    )
    .expect("the fake packages form a graph");
    (pm, probe, trace)
}
