//! The module list of a project and its restore (docs/design/digikit-project.md section 3, "Module list and restore"). The list is tag 1 of the `.dkp`: the content modules that were loaded at the stock save.
//!
//! - [`ModuleSet`]: a list of module ids with their store hashes, at most `MODULES_MAX` (the runtime's module limit). It is read from the loaded modules (a save records it) and from an
//!   image (a load restores it).
//! - [`ModuleRestore`]: after a project's data is installed, the listed modules that are on the card and not loaded are loaded live, one after the other, through the runtime's live job
//!   (`Env::live_load`, the same job as DIGIKIT > MODULES). It only adds: a loaded module that the list does not name stays. A refusal of the transport playing waits for the stop; a
//!   module that is not on the card, that the store cannot search, or that the runtime refuses, is one warning and one log line, and the next one goes on. A busy store or live job
//!   asks the same entry again in the next pass; a busy card does so after `CARD_WAIT_MS`, up to `CARD_TRIES_MAX` times. The store is read only while no live job stages (`Env::live_op`):
//!   the live job shares the staging buffer. A load that ended well is added to the saved selection, as the menu saves the selection of a module it loads; a path or a selection that
//!   cannot take it is the warning `NotKept`, and the module stays loaded. UI task only.
//!
//! The restore starts its loads in a tick after the one that installed the data, so the runtime's reload of the modules whose settings changed can take the live job first; a module
//!
//! Layout: one file per type in `restore/`: `module_id`, `module_path`, `loaded_module`, `live_job`, `module_warning`, `module_set`, `module_restore`, `event`.
mod event;
mod live_job;
mod loaded_module;
mod module_id;
mod module_path;
mod module_restore;
mod module_set;
mod module_warning;

pub use event::*;
pub use live_job::*;
pub use loaded_module::*;
pub use module_id::*;
pub use module_path::*;
pub use module_restore::*;
pub use module_set::*;
pub use module_warning::*;
