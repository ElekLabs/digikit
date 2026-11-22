//! `WorkerFns`: the job queue.
use crate::worker;
use dkrt_abi::resident::WorkerFns;
use dkrt_sdk::api::{JobDoneFn, JobStepFn};

unsafe extern "C" fn post(owner: u32, step: Option<JobStepFn>, done: Option<JobDoneFn>, ctx: *mut u8) -> i32 {
    worker::post(owner as u8, step, done, ctx)
}
unsafe extern "C" fn cancel(owner: u32, id: u32) -> i32 {
    worker::cancel(owner as u8, id)
}
unsafe extern "C" fn retire(rec: u32) -> bool {
    worker::retire(rec as usize)
}

pub(super) static FNS: WorkerFns = WorkerFns { post, cancel, retire };
