//! `project` service: the digikit data of one Elektron project, in a `.dkp` file of the store (docs/design/digikit-project.md, docs/plans/digikit-project.md).
//!
//! A crate of its own, linked into the service file (`dks/`). It reaches the store, the firmware's project list and the log only through [`engine::Env`] and [`engine::Files`],
//! so the host tests run the whole flow over fakes. Layout: [`name`] (the file names of the store from the stock project names), [`dkp`] (the file format over the record codec
//! `dkrt_sdk::settings`, the RAM image and the project scope's `get`, `put` and `remove`), [`engine`] (the state, the work after the hooks, the write job), `tests/` (host tests).
//!
//! The four stock hooks only record into [`engine::Pending`]; everything else happens in the file's tick, in the UI task.
#![no_std]
#[cfg(test)]
extern crate std;
pub mod dkp;
pub mod engine;
pub mod name;

use dkrt_abi::core_api::CoreApi;
use dkrt_abi::service::Service;

/// The project service as a `Service` row of the runtime's service table: it owns no record kind, has nothing to check and never refuses a reload (its work is the hooks, the tick and the
/// project scope, which the service file wires: `dks/`).
pub struct ProjectSvc;

impl<C: CoreApi> Service<C> for ProjectSvc {
    dkrt_abi::accepts_records!();
    dkrt_abi::accepts_teardown!();
}
