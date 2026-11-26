//! The live job's lifecycle events (`LIFECYCLE`, docs/INSTRUMENTATION.md): one begin and end for each step the job passes, and a note for each busy answer it asks again after.
use super::{step, Job, NO_REC};
use crate::package::lifecycle::{stem_hash, trace_emit, trace_on};
use dkrt_abi::package::PackageId;
use dkrt_lifecycle::{Phase, Step};
use dkrt_sdk::E_BUSY;

/// Which step of the job has a begin in the log and no end yet (`step::IDLE`: none).
#[derive(Clone, Copy)]
pub(super) struct JobTrace {
    open: u8,
}

/// The log's step of each job step, by `RELOAD_STATE` number (a table, not a `match`: the ColdFire code check reads a compiler's jump table as illegal code).
const STEPS: [Option<Step>; 7] = [None, Some(Step::JobCheck), Some(Step::JobQuiesce), Some(Step::JobUnload), Some(Step::JobLoad), Some(Step::JobDsp), Some(Step::JobDone)];

fn step_of(s: u8) -> Option<Step> {
    STEPS.get(s as usize).copied().flatten()
}

impl JobTrace {
    pub(super) const fn new() -> JobTrace {
        JobTrace { open: step::IDLE }
    }

    /// A job was queued: the next events are its own.
    pub(super) fn queued(&mut self) {
        self.open = step::IDLE;
        crate::package::lifecycle::trace_next_op();
    }

    /// The job is at `job.step`: end the step that was open and begin the new one. A finished job ends its step with the job's result and adds a note for `JobDone`.
    pub(super) fn sync(&mut self, job: &Job) {
        if !trace_on() {
            self.open = job.step;
            return;
        }
        let finished = job.phase == 2;
        if self.open == job.step && !finished {
            return;
        }
        // SAFETY: the job's path is a NUL terminated buffer of its own
        unsafe {
            let (package, hash) = (package_of(job), stem_hash(job.path.as_ptr()));
            if let Some(s) = step_of(self.open).filter(|_| self.open != step::DONE) {
                trace_emit(Phase::End, s, package, hash, if finished { job.result } else { 0 }, 0);
            }
            if finished {
                trace_emit(Phase::Note, Step::JobDone, package, hash, job.result, 0);
            } else if let Some(s) = step_of(job.step) {
                trace_emit(Phase::Begin, s, package, hash, 0, 0);
            }
        }
        self.open = if finished { step::IDLE } else { job.step };
    }

    /// The job asks its step again on a later pass because it answered `E_BUSY`.
    pub(super) fn busy(&self, job: &Job) {
        if trace_on() {
            // SAFETY: as for `sync`
            unsafe { trace_emit(Phase::Note, Step::BusyRetry, package_of(job), stem_hash(job.path.as_ptr()), E_BUSY, 0) }
        }
    }
}

fn package_of(job: &Job) -> PackageId {
    PackageId(if job.rec == NO_REC { 0xff } else { job.rec as u8 })
}
