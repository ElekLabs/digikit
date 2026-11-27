//! Live changes of the module set (docs/plans/extension-model.md E5 and E5c): what the runtime holds once for every kind of module. One job with three operations (load, unload, reload), made of the loader steps and the DSP wait (`job`), the refusal order (`refusal`), the device's static and
//! commands (`real`). Per-kind work sits behind the `Service` trait, never in here.

mod job;
mod phase;
mod real;
mod refusal;
#[cfg(test)]
mod tests;

pub use job::{step, Job, Ops};
pub use phase::phase;
pub(crate) use phase::{note_reloaded, set_phase};
pub use real::{cmd_queue, dkrt_live, dkrt_live_state, poll, state, RealOps, State};
pub use refusal::{refusal, Facts};
pub(crate) use refusal::refusal_for;
#[cfg(test)]
use real::job;
