//! The system package: what the resident core loads and runs over the package graph. The loader and the lifecycle (the package manager), the DSP transport and
//! the services' tables, the handlers of the module commands and the module admin commands, the module file and settings files, the live reload and the
//! capacity.
//!
//! The resident core reaches this folder only through the hooks (`hooks.rs`, `dkrt_abi::hooks`; one call, `hooks::install`, in `init_once`); this folder calls the
//! resident core through `crate::` paths until the resident table (S7) replaces them.

pub(crate) mod admission;
pub(crate) mod boot_idx;
pub(crate) mod boot_steps;
pub(crate) mod capacity;
pub mod collect;
pub(crate) mod commands;
pub mod core_api;
pub mod dsp;
pub mod dsp_stats;
pub mod entry;
pub(crate) mod files;
#[rustfmt::skip]
pub(crate) mod gen_services;
pub(crate) mod handlers;
// The artifact's entry symbol and panic handler: only in the `package-image` build (`[system_package]` of module.toml).
#[cfg(feature = "package-image")]
mod image;
pub(crate) mod hooks;
pub(crate) use hooks::attach;
pub(crate) mod imports;
pub(crate) mod invariants;
pub(crate) mod lifecycle;
pub(crate) mod link;
pub(crate) mod loader;
pub(crate) mod live_cmds;
pub(crate) mod module_cmds;
pub mod registry;
pub(crate) mod resident;
pub mod reload;
pub mod scan;
pub(crate) mod settings_overlay;
pub(crate) mod settings_reload;
pub(crate) mod svc_imports;
pub(crate) mod svc_rows;
#[cfg(test)]
pub(crate) mod cleanup_tests;
#[cfg(test)]
pub(crate) mod fake_checked_images;
