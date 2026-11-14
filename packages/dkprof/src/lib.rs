//! Profiles for the workbench and the CLI (docs/plans/profiling.md sections 4 and 9).
//!
//! - [`profile`]: the artifact `dk.profile/1`: run identity, clock, raw counters, metrics (a value or N/A with a reason), markers, loss and overhead.
//! - [`report`]: raw counters named through the address map into per-function and per-module self aggregates. Only self counts are added up:
//!   a raw counter is a count at one address, never an inclusive cost.
//! - [`lookup`]: the address lookup and the load table decode, as JSON in and out.
//! - [`module`]: static analysis of a module's DSP section (`dksharc`), with a text view.
//! - [`wasm`]: the C ABI of `dkprof.wasm` (JSON in and out, no bindgen), the one entry for the CLI and the browser.

pub mod lookup;
pub mod module;
pub mod profile;
pub mod report;
pub mod wasm;
