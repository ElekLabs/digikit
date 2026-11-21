//! The package manager: the one lifecycle of modules and services (docs/plans/runtime-contract-and-lifecycle.md, sections 7 and 8). It owns the graph, the
//! resource ledger, the set of live packages and the state of each, and drives a package through place, link, start, claim, install and attach on the way
//! up, and the reverse on the way down. Everything it touches outside itself is a seam ([`crate::seams`]); nothing in the runtime uses it yet (RC2b).
//!
//! - this file: the struct and its small queries.
//! - `bring_up`: `boot` and `load`, the steps of one package with the unwind of a failure, and what an operation does after a failure (continue with the
//!   packages that do not need the failed one, take down the services it started and nothing needs).
//! - `take_down`: `unload`, the take-down of one package, `retry_pending` for a busy restore.
//! - `reload`: `reload` of a module or a service (a service reloads its consumers).
//! - `resources`: the edge to the ledger and the fit plan of a bring-up, package by package.
//! - `error`, `refused`, `report`: the typed refusals and what an operation did; `progress`, `pending_take_down`: private bookkeeping.
//!
//! The DSP part of a package (plan section 8.1) is not here: it comes up over UI passes after `Installed`, so RC2b adds it as a step after `Active`
//! that takes the closure down through [`PackageManager::unload`] on a DSP failure.

use crate::graph::PackageGraph;
use crate::live::LiveSet;
use crate::seams::{Linker, Manifest, PackageInstaller, Placer, ProviderHost};
use crate::state::State;
use crate::PackageId;
use dkrt_fit::ledger::{Owner, ResourceLedger};

mod bring_up;
mod error;
mod pending_take_down;
mod progress;
mod refused;
mod reload;
mod report;
mod resources;
mod take_down;

pub use error::LifecycleError;
pub use refused::Refused;
pub use report::Report;

use pending_take_down::PendingTakeDown;
use progress::Progress;

/// The lifecycle of the packages of one selection over its seams.
pub struct PackageManager<M, P, L, I, H> {
    manifest: M,
    graph: PackageGraph,
    ledger: ResourceLedger,
    live: LiveSet,
    progress: Progress,
    pending_take_down: Option<PendingTakeDown>,
    placer: P,
    linker: L,
    installer: I,
    host: H,
}

impl<M, P, L, I, H> PackageManager<M, P, L, I, H>
where
    M: Manifest,
    P: Placer,
    L: Linker,
    I: PackageInstaller,
    H: ProviderHost,
{
    /// A manager with nothing up: the graph of the manifest's header tables and the core's pools declared.
    pub fn new(
        manifest: M,
        placer: P,
        linker: L,
        installer: I,
        host: H,
    ) -> Result<Self, LifecycleError> {
        let graph = PackageGraph::from_tables(manifest.tables(), manifest.core_apis())
            .map_err(LifecycleError::Graph)?;
        let mut ledger = ResourceLedger::new();
        for r in manifest.core_resources() {
            ledger
                .declare(Owner::Core, r)
                .map_err(LifecycleError::CoreResource)?;
        }
        Ok(PackageManager {
            manifest,
            graph,
            ledger,
            live: LiveSet::new(),
            progress: Progress::new(),
            pending_take_down: None,
            placer,
            linker,
            installer,
            host,
        })
    }

    pub fn graph(&self) -> &PackageGraph {
        &self.graph
    }

    /// The packages that are up (`Active`, or waiting for a restore).
    pub fn live(&self) -> &LiveSet {
        &self.live
    }

    pub fn ledger(&self) -> &ResourceLedger {
        &self.ledger
    }

    /// The state of `package`; `None` when it is absent.
    pub fn state_of(&self, package: PackageId) -> Option<State> {
        self.progress.state(package)
    }

    /// The package whose restore is busy or failed, if any: its take-down waits for [`PackageManager::retry_pending`], which finishes it with the packages
    /// behind it in order. Until then every other operation is refused with `RestorePending`.
    pub fn restore_pending(&self) -> Option<PackageId> {
        self.progress.pending()
    }

    pub fn manifest(&self) -> &M {
        &self.manifest
    }

    pub fn placer(&self) -> &P {
        &self.placer
    }

    pub fn linker(&self) -> &L {
        &self.linker
    }

    pub fn installer(&self) -> &I {
        &self.installer
    }

    pub fn host(&self) -> &H {
        &self.host
    }
}

#[cfg(test)]
mod tests;
