//! The service file's glue: the SDK imports the engine runs over (`Env` of `DkHost` in the UI task, `DkFiles` in the I/O task), the host state, the tick, the write job's entries and the table
//! that the runtime's settings overlay forwards to. The state is the owner's (`DK_SVC_OWNER`); the statics here are the settings overlay table and the write job's cell (`DK_JOB`, outside the owner: the I/O task works on it).
//!
//! Layout: `sdk_imports` (the SDK calls, stubs on a host build), `firmware_project` (the firmware's slot records), `host` (`DkHost`: the handles, the pass count, the listing), `env`
//! (`Env` for `DkHost`), `files` (`DkFiles`, the write job), `toast`, `c_strings`, `tick`, `scope` (the settings overlay table), `exports` (the file's exports).
mod c_strings;
mod env;
mod exports;
mod files;
mod firmware_project;
mod host;
mod scope;
mod sdk_imports;
mod tick;
mod toast;

pub use exports::{
    dk_project_current, dk_project_get, dk_project_list, dk_project_read, dk_project_settings,
    dk_project_status, DK_STATUS,
};
pub use files::{io_done, io_step, DK_JOB};
pub use host::{DkHost, Owner};
pub use scope::SCOPE;
pub use tick::tick;
