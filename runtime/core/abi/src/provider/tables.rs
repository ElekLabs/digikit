//! The function tables of the eight parts, version 1 (`row::PART_*`). A table is a `repr(C)` list of `extern "C"` functions whose first argument is the instance (the
//! owner object) and, for most, the second the core. `of::<P, C>()` builds the table of an owner object with generic thunks;
//! a service file builds it in its own object, so the table has the same layout for every service and every core. A new callback is a new version of one part.
mod describe;
mod dsp;
mod lifecycle;
mod payloads;
mod query;
mod records;
mod state;
mod tick;

pub use describe::{DescribeV1, DESCRIBE_NEWEST};
pub use dsp::{DspV1, DSP_NEWEST};
pub use lifecycle::{LifecycleV1, LIFECYCLE_NEWEST};
pub use payloads::{PayloadsV1, PAYLOADS_NEWEST};
pub use query::{QueryV1, QUERY_NEWEST};
pub use records::{RecordsV1, RECORDS_NEWEST};
pub use state::{StateV1, STATE_NEWEST};
pub use tick::{TickV1, TICK_NEWEST};
