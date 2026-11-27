//! Compile-time protobuf codecs for module queries: a `.proto` schema limited to a small subset becomes straight-line C encode and
//! decode functions (no tables, no interpreter, no third-party C). `Schema` is the checked subset; `CCodec` writes the C over the
//! helpers of `sdk/include/dk/wire.h`; `Queries` is a module's schema with its generated files (docs/plans/workbench-module-ui.md
//! "Queries").

mod budget;
mod c_codec;
mod files;
mod queries;
mod schema;

pub use budget::{REPLY_MAX, REQUEST_MAX, STACK_MAX};
pub use c_codec::{CCodec, Generated};
pub use files::WireFiles;
pub use queries::{HEADER, Queries, SOURCE};
pub use schema::Schema;
