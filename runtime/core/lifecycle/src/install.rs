//! Entries and the installer interface (docs/plans/runtime-contract-and-lifecycle.md, section 7): how a service's firmware writes are installed and
//! restored. Only the interface and the routing by CPU live here; the ColdFire and SHARC backends come in RC4. Pure, no allocation.
//!
//! - `entry`: [`Entry`] (hook, patch or slot) with the CPU-tagged [`SiteRef`] and [`SlotRef`].
//! - `error`: [`InstallError`], the typed refusals of [`Installers`] (mapped to SDK status codes at the ABI boundary in RC4).
//! - `installer`: the [`Installer`] trait: install all or nothing, idempotent restore.
//! - `installers`: [`Installers`], one installer per CPU behind the same interface.

mod entry;
mod error;
mod installer;
mod installers;

pub use entry::{Cpu, Entry, PatchBytes, SiteRef, SlotRef, MAX_PATCH};
pub use error::InstallError;
pub use installer::Installer;
pub use installers::Installers;
