//! The resident table: what the resident core gives the system package (`dkrt_abi::resident`). One file per group, named after the resident owner it fronts; each
//! holds the `extern "C"` thunks that call the owner's functions with their Rust signatures, and `table.rs` builds the one `static TABLE` from the groups.
//!
//! Nothing here names the package: the table is given to it at `hooks::enter_package` (`package::entry`), once per boot.

mod api;
mod arena;
mod boot;
mod card;
mod dlog;
mod fault;
mod imports;
mod intro;
mod io;
mod links;
mod platform;
mod sdk_files;
mod selection;
mod settings;
mod state;
mod store;
mod store_writer;
mod table;
mod transport;
mod views;
mod worker;

pub(crate) use table::TABLE;

#[cfg(test)]
mod tests;
