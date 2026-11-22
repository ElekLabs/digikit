//! The package manager: the one lifecycle of modules and services (docs/plans/runtime-contract-and-lifecycle.md, sections 7 and 8). It owns the graph, the
//! resource ledger, the set of live packages and the state of each, and drives a package through place, link, start, claim, install and attach on the way
//! up, and the reverse on the way down. Everything it touches outside itself is a seam ([`crate::seams`]); the runtime's boot and live job use it.
//!
//! - this file: the struct and its small queries.
//! - `bring_up`: `boot` and `load`, the steps of one package with the unwind of a failure, and what an operation does after a failure (continue with the
//!   packages that do not need the failed one, take down the services it started and nothing needs).
//! - `take_down`: `unload`, the take-down of one package, `retry_pending` for a busy restore.
//! - `reload`: `reload` of a module or a service (a service reloads its consumers).
//! - `resources`: the edge to the ledger and the fit plan of a bring-up, package by package.
//! - `error`, `refused`, `report`: the typed refusals and what an operation did; `progress`, `pending_take_down`: private bookkeeping.
//!
//! The DSP part of a package (plan section 8.1) comes up over UI passes after `Active`: `bring_up` asks the host to start it right after the package is up
//! (`ProviderHost::start_dsp`, one package at a time, never waiting), and a failed part takes the package's closure down ([`PackageManager::take_down_closure`]).

use crate::graph::PackageGraph;
use crate::live::LiveSet;
use crate::seams::{Linker, Manifest, PackageInstaller, Placer, ProviderHost};
use crate::state::State;
use crate::PackageId;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;
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
    /// A manager with nothing up: the graph of the manifest's header tables and the core's pools declared. It is built by value: a manager is 6 KB, so the runtime
    /// (which has little stack) uses [`PackageManager::init`].
    pub fn new(
        manifest: M,
        placer: P,
        linker: L,
        installer: I,
        host: H,
    ) -> Result<Self, LifecycleError> {
        let mut at = MaybeUninit::<Self>::uninit();
        // SAFETY: `init` writes every field before it answers `Ok`
        unsafe {
            Self::init(at.as_mut_ptr(), manifest, placer, linker, installer, host)?;
            Ok(at.assume_init())
        }
    }

    /// [`PackageManager::new`] into the place at `out`, field by field: no copy of the manager (nor of its graph and ledger) is ever on the stack. On `Err` the place holds
    /// no manager and must not be used.
    ///
    /// # Safety
    /// `out` is valid for writes of a manager and stays where it is while the manager is used; nothing reads it while this runs.
    pub unsafe fn init(
        out: *mut Self,
        manifest: M,
        placer: P,
        linker: L,
        installer: I,
        host: H,
    ) -> Result<(), LifecycleError> {
        PackageGraph::init_from_tables(
            addr_of_mut!((*out).graph),
            manifest.tables(),
            manifest.core_apis(),
        )
        .map_err(LifecycleError::Graph)?;
        ResourceLedger::init(addr_of_mut!((*out).ledger));
        for r in manifest.core_resources() {
            (*out)
                .ledger
                .declare(Owner::Core, r)
                .map_err(LifecycleError::CoreResource)?;
        }
        addr_of_mut!((*out).live).write(LiveSet::new());
        addr_of_mut!((*out).progress).write(Progress::new());
        addr_of_mut!((*out).pending_take_down).write(None);
        addr_of_mut!((*out).manifest).write(manifest);
        addr_of_mut!((*out).placer).write(placer);
        addr_of_mut!((*out).linker).write(linker);
        addr_of_mut!((*out).installer).write(installer);
        addr_of_mut!((*out).host).write(host);
        Ok(())
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

    pub fn manifest_mut(&mut self) -> &mut M {
        &mut self.manifest
    }

    /// Reads the manifest's header tables again (a package was added or its file replaced): the graph is built anew and the pins stay. Refused while a
    /// take-down waits, and when the new graph is refused (the old one stays). The live set and the states are not touched: a live package whose needs changed
    /// keeps the services it was attached to.
    pub fn refresh_graph(&mut self) -> Result<(), LifecycleError> {
        self.refuse_while_pending()?;
        // built where it stands (a graph is 1.6 KB), then it replaces the old one
        let mut slot = MaybeUninit::<PackageGraph>::uninit();
        // SAFETY: `init_from_tables` writes the graph; it is read only after it answered `Ok`, and copied over the old one (no drop, no overlap)
        unsafe {
            PackageGraph::init_from_tables(
                slot.as_mut_ptr(),
                self.manifest.tables(),
                self.manifest.core_apis(),
            )
            .map_err(LifecycleError::Graph)?;
            let graph = &mut *slot.as_mut_ptr();
            for n in self.graph.nodes() {
                if n.is_pinned() {
                    graph.set_pinned(n.id(), true);
                }
            }
            core::ptr::copy_nonoverlapping(graph, addr_of_mut!(self.graph), 1);
        }
        Ok(())
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
