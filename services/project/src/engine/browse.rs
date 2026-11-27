//! What the Project Manager reads: the list of the firmware's projects (`dk_project_list`) and the summary of one project (`dk_project_get`). UI task only.
//!
//! - [`ProjectEntry`] (`entry.rs`): one row of the list: slot, name, and the flags current, has data, data not known yet.
//! - [`DataIndex`] (`data_index.rs`): the slots that have a `.dkp`, from one listing of `projects/` after the start and from the work that changes files.
//! - [`ProjectSummary`] (`summary.rs`): the header and module list of one project, with the number of settings each module has.
//! - [`PeekCache`] (`peek.rs`): the summary of the slot asked for last. The current project is answered from its live image; another slot is read by a scan in the queue (`Work::Peek`)
//!   when the stage is idle.
//!
//! The calls are `list` and `get` of [`ProjectService`](super::ProjectService) (`list.rs`, `get.rs`).
mod data_index;
mod entry;
mod get;
mod list;
mod peek;
mod summary;

pub use data_index::DataIndex;
pub use entry::{ProjectEntry, ENTRY_CURRENT, ENTRY_DATA, ENTRY_UNKNOWN};
pub use peek::{PeekAnswer, PeekCache};
pub use summary::{ProjectModule, ProjectSummary, MODULE_LOADED, MODULE_UNKNOWN};
