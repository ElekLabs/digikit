//! The package manager of the runtime (docs/plans/runtime-contract-and-lifecycle.md, RC2), over the package graph of `dkrt-lifecycle`: the runtime's real
//! implementations of its seams. One file per seam; each is a thin adapter over the loader's own steps (`loader/package.rs`), which are the functions the old boot loops ran.
//! The boot (`boot.rs`) and the live job (`reload.rs`) run through the manager (`rt`).
//!
//! - `session`: [`Session`], the state the seams of one manager share (the loader, the checked file between place and activate, the entries of the package that just started).
//! - `placer`: [`RtPlacer`], place and free the image of a package.
//! - `linker`: [`RtLinker`], resolve the imports of a package against the services of its needs.
//! - `host`: [`RtHost`], `init` and `fini` of a module, the lifecycle calls into the rows of a service.
//! - `install`: [`RtColdFire`] and [`RtSharc`], the two backends of the `Installers` the manager installs entries through.
//! - `rt`: [`Rt`], the manager over the seams for the whole run: the boot's load and its report, the live job's load, unload and reload of a module, the DSP step.
//! - `codes`: the load status of each manager refusal (the codes the loader gave before).
//! - `manifest`: [`RtManifest`], the header tables of the selected packages and the services they need.
//!
//! A package id is a loader record (`loader/package.rs`): the module of the selection index with the same number below `MAX_MODULES`, the provider the search found
//! at that position from there.
//!
//! The unit tests of the seams are `loader/tests/seam_tests.rs` and the assembly tests over the real packed files are `assembly_tests` here (the graph from the
//! tables) and `loader/tests/manager_assembly_tests.rs` (the manager over the seams): both need the loader's own fakes and fields, which only its tests see.

mod codes;
mod host;
mod install;
mod linker;
mod manifest;
mod placer;
mod rt;
mod session;

pub(crate) use codes::code_of;
pub(crate) use host::RtHost;
pub(crate) use install::{RtColdFire, RtInstallers, RtSharc};
pub(crate) use linker::RtLinker;
pub(crate) use manifest::{CoreApis, RtManifest};
pub(crate) use placer::RtPlacer;
pub(crate) use rt::{boot, dsp_step, get};
#[cfg(test)]
pub(crate) use rt::{verdict_of, Manager, Rt};
#[cfg(test)]
pub(crate) use rt::test_up;
pub(crate) use session::Session;

#[cfg(test)]
mod assembly_tests;
