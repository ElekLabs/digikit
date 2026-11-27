//! Compile-time protobuf codecs for module queries: a `.proto` schema limited to a small subset becomes straight-line C encode and
//! decode functions (no tables, no interpreter, no third-party C). `Schema` is the checked subset; `CCodec` writes the C header
//! over the helpers of `sdk/include/dk/wire.h` (docs/plans/workbench-module-ui.md "Operations").

mod c_codec;
mod schema;

pub use c_codec::CCodec;
pub use schema::Schema;
