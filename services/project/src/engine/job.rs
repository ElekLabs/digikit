//! The write job's cell: everything the I/O task works on while a job runs (the [`StoreWriter`] steps, the [`FileBuffer`]), kept outside the service's owner.
//!
//! The service's owner is entered by the UI task for a whole call (`tick`, a scope call) while the I/O task may be running a step, so no state of a job may be a field of the
//! owner: the two tasks would hold `&mut` to overlapping memory. The cell is a static of the service file instead, and one word, the phase, says which task may touch its data:
//!
//! ```text
//! Idle --post (UI)--> Posted --first step (I/O)--> Running --done (UI, the runtime's end of the job)--> Done --poll (UI)--> Idle
//! ```
//!
//! The UI task touches the data only in `Idle` and `Done`, the I/O task only in `Posted` and `Running`. Each phase is published by one aligned word store (a volatile write of a
//! `u32`, which is one instruction on the ColdFire), after the data of that side is complete, so the other side never sees a phase before its data. All-zero bytes are the
//! initial state (`Idle`), so the cell is `.bss`.
use super::buffer::FileBuffer;
use super::writer::{JobPoll, JobSpec, StoreWriter};
use super::{Env, Files};
use crate::dkp::Image;
use core::cell::UnsafeCell;
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use dkrt_sdk::E_NOT_FOUND;

/// No job is posted; the UI task owns the data.
const IDLE: u32 = 0;
/// Posted: the I/O task owns the data, its step has not run yet.
const POSTED: u32 = 1;
/// The I/O task is in its steps.
const RUNNING: u32 = 2;
/// The runtime ended the job and `done` left the status; the UI task owns the data until it takes the end.
const DONE: u32 = 3;

struct Data {
    writer: StoreWriter,
    buffer: FileBuffer,
}

/// The cell of the one write job of the service.
pub struct StoreJob {
    phase: UnsafeCell<u32>,
    data: UnsafeCell<Data>,
}

// The phase word hands the data from one task to the other (see above).
unsafe impl Sync for StoreJob {}

impl StoreJob {
    pub const fn new() -> StoreJob {
        StoreJob {
            phase: UnsafeCell::new(IDLE),
            data: UnsafeCell::new(Data {
                writer: StoreWriter::new(),
                buffer: FileBuffer::new(),
            }),
        }
    }

    fn phase(&self) -> u32 {
        // SAFETY: an aligned word of the cell, written by one store
        unsafe { read_volatile(self.phase.get()) }
    }

    fn publish(&self, phase: u32) {
        // SAFETY: as above
        unsafe { write_volatile(self.phase.get(), phase) }
    }

    // ---- the UI task ---------------------------------------------------------------------------------------------------------------

    /// Lend the file buffer for one call of the UI task (a listing probe, a load, a read, the bytes of the next write). Only while no job is posted: the tick takes a job's end
    /// (`poll`) before it uses the buffer.
    #[allow(clippy::mut_from_ref)]
    pub fn lend(&self) -> &mut Image {
        debug_assert!(
            matches!(self.phase(), IDLE | DONE),
            "the write job holds the buffer"
        );
        // SAFETY: in `Idle` and `Done` the I/O task does not touch the data; the UI task is one task and holds one lend at a time.
        unsafe { (*addr_of_mut!((*self.data.get()).buffer)).lend() }
    }

    /// Is a job posted whose end the tick has not taken?
    pub fn is_posted(&self) -> bool {
        self.phase() != IDLE
    }

    /// Where the job stands: running, ended with a status (taken now), or none.
    pub fn poll(&self) -> JobPoll {
        match self.phase() {
            IDLE => JobPoll::None,
            DONE => {
                // SAFETY: in `Done` the data is the UI task's
                let status = unsafe { (*addr_of!((*self.data.get()).writer)).status() };
                self.publish(IDLE);
                JobPoll::Ended(status)
            }
            _ => JobPoll::Running,
        }
    }

    /// Post the job of `spec` (the bytes it writes are in the buffer). `false` when the job table is full (nothing is posted). Only while no job is posted.
    pub fn post<E: Env + ?Sized>(&self, env: &mut E, spec: JobSpec) -> bool {
        if self.phase() != IDLE {
            return false;
        }
        // SAFETY: in `Idle` the data is the UI task's
        unsafe { (*addr_of_mut!((*self.data.get()).writer)).set_spec(spec) };
        // the I/O task may run the step as soon as the runtime has the job, so the phase is published first
        self.publish(POSTED);
        if env.post() < 0 {
            self.publish(IDLE);
            return false;
        }
        true
    }

    /// The job ended (the runtime calls it in the UI task, `io_done`): `status` is the last step's result, or the runtime's cancel code. The I/O task is done with the data.
    pub fn done(&self, status: i32) {
        if !matches!(self.phase(), POSTED | RUNNING) {
            return;
        }
        // SAFETY: the job ended, so the I/O task does not touch the data any more
        unsafe { (*addr_of_mut!((*self.data.get()).writer)).set_status(status) };
        self.publish(DONE);
    }

    /// The job's open file session, if the job was cancelled in the middle (the runtime aborts it); for the host tests.
    pub fn job_handle(&self) -> i32 {
        // SAFETY: a test reads it between steps
        unsafe { (*addr_of!((*self.data.get()).writer)).job_handle() }
    }

    // ---- the I/O task --------------------------------------------------------------------------------------------------------------

    /// The write job's step (I/O task): the ops of the job in order; a busy store is `IO_MORE`, up to `BUSY_RERUNS` times, then `E_BUSY`. `E_NOT_FOUND` when no job is posted.
    pub fn io_step<F: Files + ?Sized>(&self, files: &mut F) -> i32 {
        match self.phase() {
            POSTED => self.publish(RUNNING),
            RUNNING => {}
            _ => return E_NOT_FOUND,
        }
        // SAFETY: in `Posted` and `Running` the data is the I/O task's: the UI task leaves it alone until `done`
        unsafe {
            let data = self.data.get();
            (*addr_of_mut!((*data).writer)).io_step(files, &*addr_of!((*data).buffer))
        }
    }
}

impl Default for StoreJob {
    fn default() -> Self {
        Self::new()
    }
}
