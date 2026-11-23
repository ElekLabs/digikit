//! The service contract of the runtime-contract plan (docs/plans/runtime-contract-and-lifecycle.md, section 6): what a service implements and the C ABI row
//! the core finds its parts through. The runtime drives a loaded service file's `ProviderRow` (RC3); no file here is part of the service ABI fingerprint
//! (`SVC_ABI_FILES`): a part table is versioned by its `PartRef`.
//!
//! - `contract`: [`Provider`], the lifecycle of one service object.
//! - `parts`: the optional parts [`Records`], [`DspPart`], [`Ticker`], [`Query`], [`Describe`].
//! - `tables`: the function table of each part, version 1, built from an owner object with generic thunks.
//! - `handle`: [`ProviderHandle`], a row resolved against the versions the core knows, with its instance: the calls of the core.
//! - `consumer`: [`Consumer`] and [`ConsumerId`], what `attach` and `detach` receive.
//! - `declarations`: [`Declarations`], what `declare` reports to the core.
//! - `row`: [`ProviderRow`], [`PartRef`] (plain C data) and [`check`], the only way to read a row from a file ([`CheckedRow`]).
//! - `status`, `word_sink`: the result code and the output buffer of the calls above.

mod consumer;
mod contract;
mod declarations;
mod handle;
mod parts;
mod row;
mod status;
mod tables;
mod word_sink;

pub use consumer::{Consumer, ConsumerId, Records as ConsumerRecords};
pub use contract::Provider;
pub use declarations::{Declarations, Limit, MAX_ENTRIES, MAX_RESOURCES};
pub use handle::ProviderHandle;
pub use parts::{Describe, DspPart, Query, Records, Ticker};
pub use row::{
    check, name_hash, CheckedRow, PartLookup, PartRef, ProviderRow, PART_DESCRIBE, PART_DSP,
    PART_LIFECYCLE, PART_QUERY, PART_RECORDS, PART_TICK, PROVIDER_ABI,
};
pub(crate) use status::code;
pub use status::Status;
pub use tables::*;
pub use word_sink::WordSink;
