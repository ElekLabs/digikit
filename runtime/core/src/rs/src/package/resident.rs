//! The package's view of the resident core: the only place package code reaches it. The resident core gives the package one `ResidentTable` (`dkrt_abi::resident`) at
//! `attach`, where `dkrt_abi::resident::check` decides whether it fits; each file here is a facade for one group of it, named after the resident owner, with the functions the package calls under their original names and
//! Rust signatures. Package code names `resident::<owner>::<function>` and no resident module (`check:layers`, rule `boundary-table`).
//!
//! Until the package is a file of its own (RC2s-2) it is linked into the resident binary and gets the table at the start of the first boot (`attach`); host tests
//! install it with `test_lock`.

pub(crate) mod api;
pub(crate) mod arena;
pub(crate) mod boot;
pub(crate) mod card;
pub(crate) mod dlog;
pub(crate) mod fault;
pub(crate) mod io;
pub(crate) mod intro;
pub(crate) mod platform;
pub(crate) mod sdk_files;
pub(crate) mod selection;
pub(crate) mod settings;
pub(crate) mod state;
pub(crate) mod store;
pub(crate) mod store_writer;
mod table;
pub(crate) mod transport;
pub(crate) mod worker;

pub(crate) use table::{install, table};
