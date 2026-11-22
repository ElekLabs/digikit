//! The job queue.
use super::table;
use dkrt_sdk::api::{JobDoneFn, JobStepFn};

#[inline(always)]
pub(crate) unsafe fn post(owner: u8, step: Option<JobStepFn>, done: Option<JobDoneFn>, ctx: *mut u8) -> i32 {
    (table().worker.post)(owner as u32, step, done, ctx)
}

#[inline(always)]
pub(crate) unsafe fn cancel(owner: u8, id: u32) -> i32 {
    (table().worker.cancel)(owner as u32, id)
}

/// The jobs of record `rec` are gone: false while one still runs.
#[inline(always)]
pub(crate) unsafe fn retire(rec: usize) -> bool {
    (table().worker.retire)(rec as u32)
}
