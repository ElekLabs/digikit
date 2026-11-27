//! The seams of the package manager: what it asks of the world around it (docs/plans/runtime-contract-and-lifecycle.md, section 7). The runtime
//! provides the real ones (RC2b); [`crate::fake`] provides recording ones.
//!
//! - `manifest`: [`Manifest`], the header tables and declared data of the packages.
//! - `placer`: [`Placer`], where a package's code and data live.
//! - `linker`: [`Linker`], import binding against live services.
//! - `host`: [`ProviderHost`], the calls into a package's own code (a service's parts, a module's init and fini).
//! - `install`: [`PackageInstaller`], entries into firmware, implemented by [`crate::Installers`].

mod host;
mod install;
mod linker;
mod manifest;
mod placer;

pub use host::ProviderHost;
pub use install::PackageInstaller;
pub use linker::Linker;
pub use manifest::Manifest;
pub use placer::Placer;
