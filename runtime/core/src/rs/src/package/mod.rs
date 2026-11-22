//! The system package: what the resident core loads and runs over the package graph (the loader, the lifecycle, the services' tables and the module commands).
//!
//! This folder holds the files split out of the mixed resident files; the rest of the package moves here in the next commit.

pub(crate) mod boot;
pub(crate) mod capacity;
pub(crate) mod commands;
pub(crate) mod module_admin;
