use super::{FakeHost, FakeInstaller, FakeLinker, FakeManifest, FakePlacer, Pkg, Probe};
use crate::PackageManager;
use dkrt_abi::provider::Limit;
use std::vec::Vec;

/// A package manager over the fakes.
pub type FakeManager =
    PackageManager<FakeManifest, FakePlacer, FakeLinker, FakeInstaller, FakeHost>;

/// A manager for `pkgs`, nothing up, and the probe all its seams record into. Each service declares the pools of its [`Pkg`].
pub fn manager(pkgs: &[Pkg]) -> (FakeManager, Probe) {
    let probe = Probe::default();
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
    let pm = PackageManager::new(
        FakeManifest::new(pkgs),
        FakePlacer::new(probe.clone()),
        FakeLinker::new(probe.clone()),
        FakeInstaller::new(probe.clone()),
        host,
    )
    .expect("the fake packages form a graph");
    (pm, probe)
}
