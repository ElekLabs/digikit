//! `project` service: the digikit data of one Elektron project, in a `.dkp` file of the store (docs/design/digikit-project.md, docs/plans/digikit-project.md).
//!
//! A crate of its own, linked into the service file (`dks/`). It reaches the store, the firmware's project list and the log only through [`engine::Env`] and [`engine::Files`],
//! so the host tests run the whole flow over fakes. Layout: [`name`] (the file names of the store from the stock project names), [`dkp`] (the file format over the record codec
//! `dkrt_sdk::settings`, the RAM image and the project scope's `get`, `put` and `remove`), [`engine`] (the state, the work after the hooks, the write job), `tests/` (host tests).
//!
//! [`project::Project`] is the owner object of the service (the engine and the host state of the service file, `Provider`); the file's cell holds the one instance.
//!
//! The four stock hooks only record into [`engine::Pending`]; everything else happens in the file's tick, in the UI task.
#![no_std]
#[cfg(test)]
extern crate std;
pub mod dkp;
pub mod engine;
pub mod name;
pub mod project;
