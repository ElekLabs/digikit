//! The builder calls of the machines (docs/plans/sdk.md 6.2) and their commit (6.3), as the machines service has them: the calls that name a machine, its pages and
//! their parameters (`dk_module_ctx_add_machine`, `dk_machine_add_page`, `dk_page_add_param`, `dk_param_*`, `dk_page_canvas`, `dk_page_bind*`) are the service's
//! exports (`dks/src/builder.rs`), and the state they fill is a static of this service: one module is in `init` at a time.
//!
//! `module_begin` opens the state for module record `rec`, every call appends to it, and `module_commit` checks it and lowers it to the records of the `register`
//! section (kinds PARAM, MACHINE, PAGE, ACTION and native page v1/v2, in this order) in two passes over the loader's `RecordSink` (`PREPARE` measures and checks,
//! `WRITE` writes and declares the voice rows). The loader gives the block to the registry walker, which calls this service's `check` and `apply`.
//!
//! A call never fails one by one: on overflow, on a bad argument, outside `init` or with a handle that is not ours it returns the placeholder handle (`placeholder`), the
//! first error is kept, and the setters ignore a placeholder. The error is reported at the commit. A handle is the address of an element of the state.
//!
//! Device facts (knob positions of a machine's words, stock rows by glyph, clone_of, level) come from `Profile`, read from the `dynreg_info` table.

mod apply_args;
mod builder;
mod collection;
mod commit;
mod commit_check;
mod lower;
mod open_collection;
mod profile;
mod voices;

pub use apply_args::*;
pub use builder::*;
pub use collection::*;
pub use commit::*;
pub use commit_check::*;
pub use lower::*;
pub use open_collection::*;
pub use profile::*;
pub use voices::*;
