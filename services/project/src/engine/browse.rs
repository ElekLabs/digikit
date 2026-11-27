//! What the Project Manager reads: the list of the firmware's projects (`dk_project_list`) and the summary of one project (`dk_project_get`). UI task only.
//!
//! - [`ProjectEntry`] (`entry.rs`): one row of the list: slot, name, and the flags current, has data, data not known yet.
//! - [`DataIndex`] (`data_index.rs`): the slots that have a `.dkp`, from one listing of `projects/` after the start and from the work that changes files.
//! - [`ProjectSummary`] (`summary.rs`): the header and module list of one project, with the number of settings each module has.
//! - [`PeekCache`] (`peek.rs`): the summary of the slot asked for last. The current project is answered from its live image; another slot is read by a scan in the queue (`Work::Peek`)
//!   when the stage is idle.
//!
//! - [`ProjectSetting`] (`setting.rs`): one saved setting of a module and the page reader over an image. The read of another project's file keeps one page of it (`peek.rs`).
//!
//! The calls are `list`, `get` and `settings` of [`ProjectService`](super::ProjectService) (`list.rs`, `get.rs`, `settings.rs`).
mod data_index;
mod entry;
mod get;
mod list;
mod peek;
mod setting;
mod settings;
mod summary;

pub use data_index::DataIndex;
pub use entry::{ProjectEntry, ENTRY_CURRENT, ENTRY_DATA, ENTRY_UNKNOWN};
pub use peek::{PeekAnswer, PeekCache, PAGE_MAX, PEEK_RETRY_MS};
pub use setting::{ProjectSetting, VALUE_HEAD};
pub use summary::{ProjectModule, ProjectSummary, MODULE_LOADED, MODULE_UNKNOWN};
