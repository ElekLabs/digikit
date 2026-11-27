//! The public settings of a module, from `workbench.toml`: `Schema` is the checked declaration; the descriptor bytes, the C header and
//! the TypeScript file are made from it (docs/plans/workbench-module-ui.md "Typed public settings").

mod apply;
mod c_header;
mod declaration;
mod descriptor;
#[cfg(test)]
mod fixture;
mod header;
mod ident;
mod kind;
mod label;
mod schema;
mod setting;
mod ts_file;
mod ts_schema;

pub(super) use header::Header;
pub use ts_file::TsFile;
