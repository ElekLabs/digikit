//! The package graph (docs/plans/runtime-contract-and-lifecycle.md, sections 7 and 8): packages, the services they need, and the order in which a load
//! brings packages up and an unload takes them down. Pure: no I/O, no allocation, fixed capacity from the runtime's own maxima. The package manager
//! (`manager`) owns a graph, a ledger and a live set and runs the one lifecycle over seam traits (`seams`); the runtime implements the seams (`runtime/core/src/rs/src/lifecycle`), and its boot and live job run through the manager.
//! With the `fake` feature the crate also offers recording seams and a fake manifest (`fake`) for host tests; that feature builds with `std`, so only
//! dev-dependencies enable it.
//!
//! - A **node** is one package: an id, the API it provides (a service) or none (a module), its needs and whether a selection pins it.
//! - A need is met by the one package that provides the service name with a compatible version (`dkrt_abi::version::provider_meets`). A package that emits keys for a record kind comes up after the package that declares it.
//! - Every order is deterministic: ties go by ascending [`PackageId`].
#![cfg_attr(not(any(test, feature = "fake")), no_std)]

#[cfg(any(test, feature = "fake"))]
pub mod fake;
mod graph;
mod install;
mod live;
mod manager;
mod node;
mod refusal;
mod seams;
mod state;
mod steps;
mod trace;
mod traced;

pub use dkrt_abi::package::PackageId;
use dkrt_abi::REC_MAX;
use dkrt_sdk::PROVIDER_MAX;

/// Most packages in one graph: the loader's records (selected modules and provider files).
pub const MAX_PACKAGES: usize = REC_MAX;
/// Most needs of one package: a package cannot need more services than the runtime has provider files.
pub const MAX_NEEDS: usize = PROVIDER_MAX;

pub use graph::PackageGraph;
pub use install::{
    Cpu, Entry, InstallError, Installer, Installers, PatchBytes, SiteRef, SlotRef, MAX_PATCH,
};
pub use live::LiveSet;
pub use manager::{LifecycleError, PackageManager, Refused, Report};
pub use node::{Api, Need, Node};
pub use refusal::Refusal;
pub use seams::{Linker, Manifest, PackageInstaller, Placer, ProviderHost};
pub use state::State;
pub use steps::{Steps, StepsIter};
pub use trace::{NoTrace, Phase, Step, Trace};
pub use traced::Traced;
