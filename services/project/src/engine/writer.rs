//! The store writer: the I/O job that writes and removes files. The job's step runs in the I/O task; everything else runs in the UI task. The writer only writes: the bytes
//! come from the [`FileBuffer`](super::FileBuffer) and the steps from the [`JobSpec`] a task prepared.
use super::Env;
use super::Files;
use crate::name::Path;
use dkrt_sdk::proto::{E_FILE_IN_USE, E_NOFILE};
use dkrt_sdk::{E_BUSY, OK};

/// The I/O task's return values (<dk/platform/io.h>).
pub const IO_MORE: i32 = 1;
/// A step that met a busy store this many times in a row fails with `E_BUSY`; the tick tries again after `RETRY_MS`.
pub const BUSY_RERUNS: u32 = 1000;

/// One step of the job in the I/O task.
#[derive(Clone, Copy)]
#[repr(u8)] // zero first: the service is a bss static
enum Op {
    /// An unused entry of the table.
    Unused,
    /// Write `len` bytes of the file buffer as `path`.
    Write {
        path: Path,
        len: u32,
    },
    Delete {
        path: Path,
    },
}

const OP_MAX: usize = 4;

/// The steps of one job, as a task prepares them: a write and up to three removals, run in order.
pub struct JobSpec {
    ops: [Op; OP_MAX],
    count: usize,
}

impl JobSpec {
    pub const fn new() -> JobSpec {
        JobSpec {
            ops: [Op::Unused; OP_MAX],
            count: 0,
        }
    }
    /// The next step writes `len` bytes of the buffer as `path`.
    pub fn write(&mut self, path: Path, len: u32) {
        self.push(Op::Write { path, len });
    }
    /// The next step removes `path`.
    pub fn delete(&mut self, path: Path) {
        self.push(Op::Delete { path });
    }
    fn push(&mut self, op: Op) {
        if self.count < OP_MAX {
            self.ops[self.count] = op;
            self.count += 1;
        }
    }
}

impl Default for JobSpec {
    fn default() -> Self {
        Self::new()
    }
}

/// Where the job is. Only the UI task changes it, except that `io_done` (the runtime calls it in the UI task) ends a job.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)] // zero first: the service is a bss static
enum JobPhase {
    /// No job is posted.
    Idle,
    /// Posted: the I/O task runs it.
    Running,
    /// `io_done` ran and left `status`; the tick has not taken it.
    Ended,
}

/// What the I/O task runs, and what it left.
struct Job {
    spec: JobSpec,
    /// The step the I/O task is at.
    op_at: usize,
    /// The open file session of the current `Write` (a handle is above 0), or 0 for none.
    handle: i32,
    reruns: u32,
    phase: JobPhase,
    status: i32,
}

impl Job {
    const fn new() -> Job {
        Job {
            spec: JobSpec::new(),
            op_at: 0,
            handle: 0,
            reruns: 0,
            phase: JobPhase::Idle,
            status: 0,
        }
    }
}

/// The state of a job the tick looks at.
pub enum JobPoll {
    /// No job is posted.
    None,
    /// The job runs.
    Running,
    /// The job ended with this status (taken: the next poll says `None`).
    Ended(i32),
}

/// Owns the posted job and runs its steps. The tick posts a job per task of the run in progress (UI task); the I/O task runs `io_step`; `io_done` hands the result back to the tick.
pub struct StoreWriter {
    job: Job,
}

impl StoreWriter {
    pub const fn new() -> StoreWriter {
        StoreWriter { job: Job::new() }
    }

    // ---- the job (UI task) -----------------------------------------------------------------------------------------------------------

    /// Is a job posted whose end the tick has not taken?
    pub fn is_posted(&self) -> bool {
        self.job.phase != JobPhase::Idle
    }

    /// Where the job stands: running, ended with a status (taken now), or none.
    pub fn poll(&mut self) -> JobPoll {
        if self.job.phase == JobPhase::Idle {
            return JobPoll::None;
        }
        if self.job.phase == JobPhase::Running {
            return JobPoll::Running;
        }
        self.job.phase = JobPhase::Idle;
        JobPoll::Ended(self.job.status)
    }

    /// Post the job of `spec`. `false` when the job table is full (nothing is posted).
    pub fn post<E: Env + ?Sized>(&mut self, env: &mut E, spec: JobSpec) -> bool {
        self.job = Job::new();
        self.job.spec = spec;
        if env.post() < 0 {
            return false;
        }
        self.job.phase = JobPhase::Running;
        true
    }

    // ---- the job (I/O task) ----------------------------------------------------------------------------------------------------------

    /// The write job's step (I/O task): the ops of the job in order; a busy store is `IO_MORE`, up to `BUSY_RERUNS` times, then `E_BUSY`. `buffer` is the file buffer's job bytes.
    pub fn io_step<F: Files + ?Sized>(&mut self, files: &mut F, buffer: &super::FileBuffer) -> i32 {
        loop {
            if self.job.op_at >= self.job.spec.count {
                return OK;
            }
            let op = self.job.spec.ops[self.job.op_at];
            if let Op::Delete { path } = op {
                let status = files.delete(path.as_bytes());
                if status == OK || status == E_NOFILE {
                    self.job.op_at += 1;
                    self.job.reruns = 0;
                    continue;
                }
                return self.busy_or(status);
            }
            if let Op::Write { path, len } = op {
                if self.job.handle == 0 {
                    let handle = files.create(path.as_bytes(), len);
                    if handle < 0 {
                        return self.busy_or(handle);
                    }
                    self.job.handle = handle;
                }
                let handle = self.job.handle;
                let status = files.write(handle, buffer.job_bytes(len as usize));
                if status == E_BUSY {
                    // nothing was written: the same call again
                    return self.busy_or(status);
                }
                if status != OK {
                    files.abort(handle);
                    self.job.handle = 0;
                    return status;
                }
                let closed = files.close(handle);
                self.job.handle = 0;
                if closed == OK {
                    self.job.op_at += 1;
                    self.job.reruns = 0;
                    continue;
                }
                // the session is closed whatever the result: a busy store starts the write again
                return self.busy_or(closed);
            }
            self.job.op_at += 1;
        }
    }

    /// A busy or in-use answer is a rerun (up to `BUSY_RERUNS`); another status is returned as it is.
    fn busy_or(&mut self, status: i32) -> i32 {
        if status == E_BUSY || status == E_FILE_IN_USE {
            self.job.reruns += 1;
            if self.job.reruns > BUSY_RERUNS {
                return E_BUSY;
            }
            return IO_MORE;
        }
        status
    }

    /// The job ended (UI task): `status` is the last step's result, or the runtime's cancel code.
    pub fn io_done(&mut self, status: i32) {
        self.job.status = status;
        self.job.phase = JobPhase::Ended;
    }

    /// The job's open file session, if the job was cancelled in the middle (the runtime aborts it); for the host tests.
    pub fn job_handle(&self) -> i32 {
        self.job.handle
    }
}

impl Default for StoreWriter {
    fn default() -> Self {
        Self::new()
    }
}
