//! The service contract of the runtime-contract plan (docs/plans/runtime-contract-and-lifecycle.md, section 6): what a service implements and the C ABI row
//! the core finds its parts through. Nothing in the runtime uses it yet (RC3 wires it); no file here is part of the service ABI fingerprint (`SVC_ABI_FILES`).
//!
//! - `contract`: [`Provider`], the lifecycle of one service object.
//! - `parts`: the optional parts [`Records`], [`DspPart`], [`Ticker`], [`Query`], [`Describe`].
//! - `consumer`: [`Consumer`] and [`ConsumerId`], what `attach` and `detach` receive.
//! - `declarations`: [`Declarations`], what `declare` reports to the core.
//! - `row`: [`ProviderRow`], [`PartRef`] (plain C data) and [`check`], the only way to read a row from a file ([`CheckedRow`]).
//! - `status`, `word_sink`: the result code and the output buffer of the calls above.

mod consumer;
mod contract;
mod declarations;
mod parts;
mod row;
mod status;
mod word_sink;

pub use consumer::{Consumer, ConsumerId, Records as ConsumerRecords};
pub use contract::Provider;
pub use declarations::{Declarations, Limit, MAX_ENTRIES, MAX_RESOURCES};
pub use parts::{Describe, DspPart, Query, Records, Ticker};
pub use row::{
    check, CheckedRow, PartLookup, PartRef, ProviderRow, PART_DESCRIBE, PART_DSP, PART_LIFECYCLE,
    PART_QUERY, PART_RECORDS, PART_TICK, PROVIDER_ABI,
};
pub use status::Status;
pub use word_sink::WordSink;
