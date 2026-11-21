//! The package graph (docs/plans/runtime-contract-and-lifecycle.md, sections 7 and 8): packages, the services they need, and the order in which a load
//! brings packages up and an unload takes them down. Pure: no I/O, no allocation, fixed capacity from the runtime's own maxima. The package manager
//! (RC2) will own a graph and a live set; nothing in the runtime uses this crate yet.
//!
//! - A **node** is one package: an id, the API it provides (a service) or none (a module), its needs and whether a selection pins it.
//! - A need is met by the one package that provides the service name with a compatible version (`dkrt_abi::version::version_satisfies`).
//! - Every order is deterministic: ties go by ascending [`PackageId`].
#![no_std]

mod graph;
mod live;
mod node;
mod refusal;
mod steps;

pub use dkrt_abi::package::PackageId;
use dkrt_abi::REC_MAX;
use dkrt_sdk::PROVIDER_MAX;

/// Most packages in one graph: the loader's records (selected modules and provider files).
pub const MAX_PACKAGES: usize = REC_MAX;
/// Most needs of one package: a package cannot need more services than the runtime has provider files.
pub const MAX_NEEDS: usize = PROVIDER_MAX;

pub use graph::PackageGraph;
pub use live::LiveSet;
pub use node::{Api, Need, Node};
pub use refusal::Refusal;
pub use steps::{Steps, StepsIter};
