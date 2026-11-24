//! The project that is current: its slot, the live image with its bookkeeping, the file it came from. UI task only (the project scope's `get`, `put` and `remove` run there too).
//!
//! Layout: `slot`, `source_file`, `applied`, `bound_view`, `binding` (the small types), `project` (`BoundProject` and its accessors) with its behaviour in `project/` (`scope`,
//! `install`, `bind`, `freeze`).
mod applied;
mod binding;
mod bound_view;
mod project;
mod slot;
mod source_file;

pub use applied::*;
use binding::*;
pub use bound_view::*;
pub use project::*;
pub use slot::*;
pub use source_file::*;
