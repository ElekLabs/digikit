//! The package manager of the runtime (docs/plans/runtime-contract-and-lifecycle.md, RC2), over the package graph of `dkrt-lifecycle`: the runtime's real
//! implementations of its seams. One file per seam; each is a thin adapter over the loader's own steps (`loader/package.rs`), which are the functions the boot's
//! loops run. Nothing in the runtime uses them yet: the boot and the reload job stay as they are until RC2b-2.
//!
//! - `session`: [`Session`], the state the seams of one manager share (the loader, the checked file between place and activate, the entries of the package that just started).
//! - `placer`: [`RtPlacer`], place and free the image of a package.
//! - `linker`: [`RtLinker`], resolve the imports of a package against the services of its needs.
//! - `host`: [`RtHost`], `init` and `fini` of a module, the lifecycle calls into the rows of a service.
//! - `install`: [`RtColdFire`] and [`NoSharc`], the two backends of the `Installers` the manager installs entries through.
//! - `manifest`: [`RtManifest`], the header tables of the selected packages and the services they need.
//!
//! A package id is a loader record (`loader/package.rs`): the module of the selection index with the same number below `MAX_MODULES`, the provider the search found
//! at that position from there.
//!
//! The unit tests of the seams are `loader/tests/seam_tests.rs` and the assembly tests over the real packed files are `assembly_tests` here (the graph from the
//! tables) and `loader/tests/manager_assembly_tests.rs` (the manager over the seams): both need the loader's own fakes and fields, which only its tests see.
// the seams are used by RC2b-2: until then only the tests call them
#![allow(dead_code, unused_imports)]

mod host;
mod install;
mod linker;
mod manifest;
mod placer;
mod session;

pub(crate) use host::RtHost;
pub(crate) use install::{NoSharc, RtColdFire, RtInstallers};
pub(crate) use linker::RtLinker;
pub(crate) use manifest::RtManifest;
pub(crate) use placer::RtPlacer;
pub(crate) use session::Session;

#[cfg(test)]
mod assembly_tests;
