//! The write job's cell (`StoreJob`): the phase word hands the job's data between the UI task and the I/O task, and the I/O task needs no owner. All-zero bytes are the idle cell.
mod host;
use dkrt_sdk::{E_BUSY, E_NOT_FOUND, OK};
use dkrt_svc_project::engine::{Files, JobPoll, JobSpec, StoreJob, IO_MORE};
use host::{TestHost, POST_STATUS};
use std::mem::MaybeUninit;

/// A store that answers busy a few times and then takes the file.
struct BusyThenOk {
    busy: u32,
    wrote: Vec<u8>,
}

impl Files for BusyThenOk {
    fn create(&mut self, _: &[u8], _: u32) -> i32 {
        if self.busy > 0 {
            self.busy -= 1;
            return E_BUSY;
        }
        1
    }
    fn write(&mut self, _: i32, buf: &[u8]) -> i32 {
        self.wrote = buf.to_vec();
        OK
    }
    fn close(&mut self, _: i32) -> i32 {
        OK
    }
    fn abort(&mut self, _: i32) {}
    fn delete(&mut self, _: &[u8]) -> i32 {
        OK
    }
}

fn spec(len: u32) -> JobSpec {
    let mut s = JobSpec::new();
    s.write(
        dkrt_svc_project::name::Path::from_bytes(b"projects/1-A.dkp").unwrap(),
        len,
    );
    s
}

#[test]
fn the_phases_hand_the_job_over_in_order() {
    let job = StoreJob::new();
    let mut env = TestHost;
    assert!(matches!(job.poll(), JobPoll::None));
    // the UI task fills the buffer, then posts
    job.lend().0[..3].copy_from_slice(b"abc");
    assert!(job.post(&mut env, spec(3)));
    assert!(job.is_posted());
    assert!(
        matches!(job.poll(), JobPoll::Running),
        "posted: the job runs"
    );
    assert!(
        !job.post(&mut env, spec(3)),
        "a second job waits for the end of the first"
    );
    // the I/O task: busy answers are reruns, then the write goes out with the bytes the UI task left
    let mut files = BusyThenOk {
        busy: 2,
        wrote: Vec::new(),
    };
    assert_eq!(job.io_step(&mut files), IO_MORE);
    assert!(
        matches!(job.poll(), JobPoll::Running),
        "running: still the I/O task's"
    );
    assert_eq!(job.io_step(&mut files), IO_MORE);
    assert_eq!(job.io_step(&mut files), OK);
    assert_eq!(files.wrote, b"abc");
    assert!(
        matches!(job.poll(), JobPoll::Running),
        "the step ended but the runtime has not ended the job"
    );
    // the runtime ends the job in the UI task; the UI task takes the end once
    job.done(OK);
    assert!(matches!(job.poll(), JobPoll::Ended(OK)));
    assert!(matches!(job.poll(), JobPoll::None));
    assert!(!job.is_posted());
    // the buffer is the UI task's again
    assert_eq!(&job.lend().0[..3], b"abc");
}

#[test]
fn a_step_without_a_posted_job_touches_nothing_and_fails() {
    let job = StoreJob::new();
    let mut files = BusyThenOk {
        busy: 0,
        wrote: Vec::new(),
    };
    assert_eq!(job.io_step(&mut files), E_NOT_FOUND);
    // a job that ended: the I/O task has no more steps to run
    let mut env = TestHost;
    assert!(job.post(&mut env, spec(1)));
    job.done(E_BUSY);
    assert_eq!(
        job.io_step(&mut files),
        E_NOT_FOUND,
        "done: the data is the UI task's"
    );
    assert!(files.wrote.is_empty());
    assert!(matches!(job.poll(), JobPoll::Ended(E_BUSY)));
}

#[test]
fn an_end_with_no_job_is_ignored() {
    let job = StoreJob::new();
    job.done(OK);
    assert!(matches!(job.poll(), JobPoll::None));
    assert!(!job.is_posted());
}

#[test]
fn a_refused_post_leaves_the_cell_idle() {
    let job = StoreJob::new();
    let mut env = TestHost;
    POST_STATUS.with(|s| s.set(-1));
    assert!(!job.post(&mut env, spec(1)), "the job table is full");
    POST_STATUS.with(|s| s.set(0));
    assert!(!job.is_posted());
    assert!(matches!(job.poll(), JobPoll::None));
    assert!(job.post(&mut env, spec(1)), "the next post goes through");
}

#[test]
fn a_zeroed_cell_is_the_idle_cell() {
    let zeroed = Box::new(MaybeUninit::<StoreJob>::zeroed());
    // SAFETY: the cell is all-zero bytes when idle (`.bss`); this test checks it by behaviour
    let job = unsafe { zeroed.assume_init_ref() };
    assert!(matches!(job.poll(), JobPoll::None));
    assert!(!job.is_posted());
    let mut env = TestHost;
    assert!(job.post(&mut env, spec(1)));
}

// the check is a debug assertion
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "the write job holds the buffer")]
fn the_ui_task_may_not_use_the_buffer_while_a_job_is_posted() {
    let job = StoreJob::new();
    let mut env = TestHost;
    assert!(job.post(&mut env, spec(1)));
    let _ = job.lend();
}
