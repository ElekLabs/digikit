//! The service file's glue: the SDK imports the engine runs over (`DkEnv` in the UI task, `DkFiles` in the I/O task), the state, the tick, the write job's entries and the table
//! that the runtime's settings overlay forwards to. All state is static: one service, one file.
//!
//! Layout: `sdk_imports` (the SDK calls, stubs on a host build), `firmware_project` (the firmware's slot records), `service_state` (the statics), `env` (`DkEnv`), `files`
//! (`DkFiles`, the write job), `toast`, `c_strings`, `setup`, `tick`, `scope` (the settings overlay table), `exports` (the file's exports).
mod c_strings;
mod env;
mod exports;
mod files;
mod firmware_project;
mod scope;
mod sdk_imports;
mod service_state;
mod setup;
mod tick;
mod toast;

pub use exports::{dk_project_current, dk_project_status};
pub use setup::setup;
pub use tick::tick;
