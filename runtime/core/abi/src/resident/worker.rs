//! `WorkerFns`: the job queue (`worker.rs`).
use dkrt_sdk::api::{JobDoneFn, JobStepFn};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct WorkerFns {
    pub post: unsafe extern "C" fn(
        owner: u32,
        step: Option<JobStepFn>,
        done: Option<JobDoneFn>,
        ctx: *mut u8,
    ) -> i32,
    pub cancel: unsafe extern "C" fn(owner: u32, id: u32) -> i32,
    /// The jobs of record `rec` are gone: false while one still runs.
    pub retire: unsafe extern "C" fn(rec: u32) -> bool,
}
