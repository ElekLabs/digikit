//! The live job's lifecycle events (`LIFECYCLE`, docs/INSTRUMENTATION.md): one begin and end for each step the job passes, and a note for each busy answer it asks again after.
use super::{step, Job, NO_REC};
use crate::package::lifecycle::{stem_hash, trace_emit, trace_on, TRACE_NO_PACKAGE};
use dkrt_abi::package::PackageId;
use dkrt_lifecycle::{Phase, Step};
use dkrt_sdk::E_BUSY;

/// Which step of the job has a begin in the log and no end yet (`step::IDLE`: none).
#[derive(Clone, Copy)]
pub(super) struct JobTrace {
    open: u8,
    /// The operation number of the job, taken when it was queued: every event of the job carries it, whatever else runs.
    op: u32,
}

/// The log's step of each job step, by `RELOAD_STATE` number (a table, not a `match`: the ColdFire code check reads a compiler's jump table as illegal code).
const STEPS: [Option<Step>; 7] = [None, Some(Step::JobCheck), Some(Step::JobQuiesce), Some(Step::JobUnload), Some(Step::JobLoad), Some(Step::JobDsp), Some(Step::JobDone)];

fn step_of(s: u8) -> Option<Step> {
    STEPS.get(s as usize).copied().flatten()
}

/// The events one `sync` adds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Plan {
    end: Option<Step>,
    begin: Option<Step>,
    note: bool,
}

impl Plan {
    const NONE: Plan = Plan { end: None, begin: None, note: false };
}

impl JobTrace {
    pub(super) const fn new() -> JobTrace {
        JobTrace { open: step::IDLE, op: 0 }
    }

    /// A job was queued: the next events are its own.
    pub(super) fn queued(&mut self, op: u32) {
        self.open = step::IDLE;
        self.op = op;
    }

    /// The operation number of the job.
    pub(super) fn op(&self) -> u32 {
        self.op
    }

    /// What the log gets when the job is at `at`: the end of the step whose begin the log has, then the begin of the new step, or for a `finished` job a note for `JobDone`.
    /// `on` is false while nothing listens: nothing is emitted, and no step counts as begun, so a sink that attaches in the middle of a step gets no end without its begin.
    pub(super) fn plan(&mut self, on: bool, at: u8, finished: bool) -> Plan {
        if !on {
            self.open = step::IDLE;
            return Plan::NONE;
        }
        if self.open == at && !finished {
            return Plan::NONE;
        }
        let end = step_of(self.open).filter(|_| self.open != step::DONE);
        let begin = if finished { None } else { step_of(at) };
        self.open = if finished { step::IDLE } else if begin.is_some() { at } else { step::IDLE };
        Plan { end, begin, note: finished }
    }

    /// The job is at `job.step`: log the plan for it.
    pub(super) fn sync(&mut self, job: &Job) {
        let finished = job.phase == 2;
        let plan = self.plan(trace_on(), job.step, finished);
        if plan.end.is_none() && plan.begin.is_none() && !plan.note {
            return;
        }
        // SAFETY: the job's path is a NUL terminated buffer of its own
        unsafe {
            let (package, hash) = (package_of(job), stem_hash(job.path.as_ptr()));
            if let Some(s) = plan.end {
                trace_emit(self.op, Phase::End, s, package, hash, if finished { job.result } else { 0 }, 0);
            }
            if plan.note {
                trace_emit(self.op, Phase::Note, Step::JobDone, package, hash, job.result, 0);
            }
            if let Some(s) = plan.begin {
                trace_emit(self.op, Phase::Begin, s, package, hash, 0, 0);
            }
        }
    }

    /// The job asks its step again on a later pass because it answered `E_BUSY`.
    pub(super) fn busy(&self, job: &Job) {
        if trace_on() {
            // SAFETY: as for `sync`
            unsafe { trace_emit(self.op, Phase::Note, Step::BusyRetry, package_of(job), stem_hash(job.path.as_ptr()), E_BUSY, 0) }
        }
    }
}

fn package_of(job: &Job) -> PackageId {
    if job.rec == NO_REC { TRACE_NO_PACKAGE } else { PackageId(job.rec as u8) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_ends_only_when_its_begin_was_logged() {
        let mut t = JobTrace::new();
        // the sink is off during the check: nothing is logged, nothing is open
        assert_eq!(t.plan(false, step::CHECK, false), Plan::NONE);
        // it attaches during the quiesce: that step begins, and the check has no end
        let p = t.plan(true, step::QUIESCE, false);
        assert_eq!((p.end, p.begin), (None, Some(Step::JobQuiesce)));
        let p = t.plan(true, step::UNLOAD, false);
        assert_eq!((p.end, p.begin), (Some(Step::JobQuiesce), Some(Step::JobUnload)));
        // the same step again: no event
        assert_eq!(t.plan(true, step::UNLOAD, false), Plan::NONE);
        let p = t.plan(true, step::DONE, true);
        assert_eq!((p.end, p.begin, p.note), (Some(Step::JobUnload), None, true));
    }

    #[test]
    fn a_sink_that_attaches_before_the_end_of_the_job_gets_no_end() {
        let mut t = JobTrace::new();
        t.plan(false, step::UNLOAD, false);
        let p = t.plan(true, step::DONE, true);
        assert_eq!((p.end, p.begin, p.note), (None, None, true));
    }
}
