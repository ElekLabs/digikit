//! Lifecycle tracing (docs/INSTRUMENTATION.md "Lifecycle events"): every call the manager makes through a seam is announced before it runs and answered after it, so a log that
//! shows a begin without its end shows where a lifecycle step hangs. The manager knows nothing of it: [`Traced`](crate::Traced) wraps a seam and calls a [`Trace`].
//!
//! - `phase`: [`Phase`], begin, end or a note.
//! - `step`: [`Step`], what the call is.
//! - this file: [`Trace`], the sink the runtime implements over its event log, and [`NoTrace`], the default that does nothing.

mod phase;
mod step;

pub use phase::Phase;
pub use step::Step;

use crate::PackageId;

/// Where lifecycle events go. A trace is cheap to copy (the four wrapped seams of one manager each hold one) and takes `&self`: its state, if any, is shared.
pub trait Trace: Clone {
    /// Whether events are wanted now. A wrapper asks first and skips the event when it is `false`, so a producer pays this one test while nothing listens.
    fn on(&self) -> bool;

    /// One event: `step` of `package` began, ended with `status` (0 is success; the status the call answered), or is a note.
    fn event(&self, phase: Phase, step: Step, package: PackageId, status: i32);
}

/// The trace that is off: no event is ever produced.
#[derive(Clone, Copy, Default)]
pub struct NoTrace;

impl Trace for NoTrace {
    fn on(&self) -> bool {
        false
    }

    fn event(&self, _phase: Phase, _step: Step, _package: PackageId, _status: i32) {}
}
