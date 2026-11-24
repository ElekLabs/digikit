//! Peak windows for `sample_pool_peaks_window`: min/max of `columns` equal parts of a frame range of a READY pool slot. The PCM is
//! read in the worker task (`worker.rs`) through the optional adapter hook `dkpool_window_step` (dkrt_pool.h); the UI task only
//! starts, polls and copies.
//!
//! One window per sample-tracks client. A request is a key (slot index, token, first frame, frame count, columns). States (`Window`, in the owner, UI task only):
//!   Idle    nothing requested, or the last result is not valid any more
//!   Pending a job runs for `cur`; `next` holds a newer request when `has_next`
//!   Ready   the job's cell holds the pairs of `cur`
//!   Failed  the job ended with an error; it is returned until the key changes (no retry)
//!
//! The job's data is not in the owner. The worker task reads the request and writes the result and the cursor while the UI task may be inside the owner, so the data of a job is a
//! [`PeakJob`], one cell for each client in [`PeakJobs`] (a static of the service file), and one phase word hands it between the tasks:
//!
//! ```text
//! Idle --start (UI)--> Posted --first step (worker)--> Running --end of the job (UI)--> Done --start (UI)--> Posted ...
//! ```
//!
//! The UI task touches a cell only in `Idle` and `Done`, the worker task only in `Posted` and `Running`; each phase is one aligned word store through a [`Handoff`] (a volatile `u32` with a release fence before the store and an acquire fence after a read). A new key while a
//! job runs goes to `next`; the old job is cancelled and, when its end arrives as a `PeakDone` event (`Samples::peak_done`), the job for `next` starts. So a window never has two jobs.
use crate::events::PoolEvent;
use crate::pool::{Pool, WindowStepFn};
use crate::tracks::{TrackCore, Tracks, CLIENTS};
use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use dkrt_abi::core_api::{CoreApi, JOB_E_CANCELLED};
use dkrt_abi::handoff::Handoff;
use dkrt_sdk::api::JobDoneFn;
use dkrt_sdk::sample_tracks::{Handle, PATH};
use dkrt_sdk::{E_CONTRACT, E_RANGE};

pub const WINDOW_MAX: usize = 128;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Key {
    index: u32,
    token: u32,
    first: u32,
    frames: u32,
    columns: u32,
}

/// A request: the key, a copy of the backing handle and the NUL-terminated path the adapter gets.
#[derive(Clone, Copy)]
pub(crate) struct Req {
    key: Key,
    handle: Handle,
    path: [u8; PATH + 1],
}

const KEY0: Key = Key {
    index: 0,
    token: 0,
    first: 0,
    frames: 0,
    columns: 0,
};

const REQ0: Req = Req {
    key: KEY0,
    handle: Handle {
        id: 0,
        token: 0,
        base_l: 0,
        base_r: 0,
        frames: 0,
        rate: 0,
        channels: 0,
    },
    path: [0; PATH + 1],
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum State {
    Idle,
    Pending,
    Ready,
    Failed(i32),
}

/// The window of one client: its state machine, in the owner (UI task only). The job's data is in the client's [`PeakJob`].
pub(crate) struct Window {
    client: usize,
    state: State,
    job: u32,
    cur: Key,
    next: Req,
    has_next: bool,
    /// The client went away while Pending: the end of the job ends in Idle.
    abandon: bool,
}

const WINDOW0: Window = Window {
    client: 0,
    state: State::Idle,
    job: 0,
    cur: KEY0,
    next: REQ0,
    has_next: false,
    abandon: false,
};

/// The peak windows of the owner, one per client.
pub(crate) struct PeakWindows {
    win: [Window; CLIENTS],
}

// ---- the job's cell, outside the owner ----------------------------------------------------------------------------------------------

/// No job: the UI task owns the cell.
const IDLE: u32 = 0;
/// Posted: the worker task owns the cell, its step has not run yet.
const POSTED: u32 = 1;
/// The worker task is in its steps.
const RUNNING: u32 = 2;
/// The job ended (the runtime called `end` in the UI task): the UI task owns the cell again.
const DONE: u32 = 3;

/// What the worker reads and writes (and the end's status), apart from the phase.
struct JobData {
    client: u32,
    key: Key,
    handle: Handle,
    path: [u8; PATH + 1],
    /// The adapter's `dkpool_window_step` hook.
    hook: Option<WindowStepFn>,
    /// The job-end entry, kept for the job that follows this one on the same window.
    done: Option<JobDoneFn>,
    /// `columns` pairs of int16 min, max; written by the worker.
    result: [i16; 2 * WINDOW_MAX],
    /// Next frame to read; written by the worker.
    cursor: u32,
    /// What the job ended with (`end`).
    status: i32,
}

/// The data of one window's job and the phase word that hands it between the UI task and the worker task. All-zero bytes are the idle cell (`.bss`).
pub struct PeakJob {
    phase: Handoff,
    data: UnsafeCell<JobData>,
}

// The phase word hands the data from one task to the other (see the module doc).
unsafe impl Sync for PeakJob {}

impl PeakJob {
    const fn new() -> PeakJob {
        PeakJob {
            phase: Handoff::new(),
            data: UnsafeCell::new(JobData {
                client: 0,
                key: KEY0,
                handle: REQ0.handle,
                path: [0; PATH + 1],
                hook: None,
                done: None,
                result: [0; 2 * WINDOW_MAX],
                cursor: 0,
                status: 0,
            }),
        }
    }

    /// The job ended (the runtime calls the job-end entry in the UI task): the status is kept, the UI task owns the cell again, and the event that carries the end to the owner is
    /// returned. The file's entry posts it to its cell (`Owner::post`), so the window changes under the owner's guard.
    ///
    /// # Safety
    /// `ctx` is the pointer the job was posted with (a `PeakJob`).
    pub unsafe fn end(ctx: *mut u8, status: i32) -> PoolEvent {
        let job = &*(ctx as *const PeakJob);
        // the job ended, so the worker does not touch the data any more
        write_volatile(addr_of_mut!((*job.data.get()).status), status);
        job.phase.publish(DONE);
        PoolEvent::PeakDone {
            job: job as *const PeakJob,
        }
    }

    /// Test support: the phase.
    #[cfg(test)]
    pub(crate) fn phase_of(&self) -> u32 {
        self.phase.phase()
    }
}

/// The jobs' cells, one for each client: a static of the service file, never a field of the owner.
pub struct PeakJobs {
    job: [PeakJob; CLIENTS],
}

impl PeakJobs {
    pub const fn new() -> PeakJobs {
        PeakJobs {
            job: [const { PeakJob::new() }; CLIENTS],
        }
    }
}

impl Default for PeakJobs {
    fn default() -> Self {
        Self::new()
    }
}

/// Worker task. Calls the adapter for one chunk and returns its value (1: more). It touches only the cell, and only while the phase is Posted or Running.
unsafe extern "C" fn step(ctx: *mut u8) -> i32 {
    let job = &*(ctx as *const PeakJob);
    match job.phase.phase() {
        POSTED => job.phase.publish(RUNNING),
        RUNNING => {}
        _ => return E_CONTRACT,
    }
    let d = job.data.get();
    match read_volatile(addr_of!((*d).hook)) {
        Some(hook) => hook(
            addr_of!((*d).handle),
            addr_of!((*d).path) as *const u8,
            (*d).key.first,
            (*d).key.frames,
            (*d).key.columns,
            addr_of_mut!((*d).result) as *mut i16,
            addr_of_mut!((*d).cursor),
        ),
        None => E_CONTRACT,
    }
}

// ---- state machine ----------------------------------------------------------------------------------------------------------------

/// The arguments that need no slot: `out` aligned, `columns` in range, enough capacity, a non-empty range.
fn check_args(out: *const u8, frames: u32, columns: u32, capacity: u32) -> Result<(), i32> {
    if out.is_null()
        || (out as usize & 1) != 0
        || columns < 1
        || columns as usize > WINDOW_MAX
        || capacity < columns * 4
        || frames < 1
    {
        return Err(E_CONTRACT);
    }
    Ok(())
}

/// Does the slot of the key still hold the same load?
fn alive(tracks: &TrackCore, client: usize, key: &Key) -> bool {
    tracks
        .peak_handle(client, key.index as usize, key.token)
        .is_ok()
}

/// What `start` needs besides the request: the adapter's hook and the job-end entry (the file's: it posts the end to the owner).
#[derive(Clone, Copy)]
pub(crate) struct Link {
    pub(crate) hook: WindowStepFn,
    pub(crate) done: JobDoneFn,
}

/// Zero the result, set the cursor and post the job for `req`. 0, or the error of `job_post` (the state is Idle then).
fn start<C: CoreApi>(core: &C, cell: &PeakJob, w: &mut Window, req: &Req, link: Link) -> i32 {
    debug_assert!(
        matches!(cell.phase.phase(), IDLE | DONE),
        "a job runs in this cell"
    );
    w.cur = req.key;
    // SAFETY: in `Idle` and `Done` the cell is the UI task's
    unsafe {
        let d = cell.data.get();
        write_volatile(addr_of_mut!((*d).client), w.client as u32);
        write_volatile(addr_of_mut!((*d).key), req.key);
        write_volatile(addr_of_mut!((*d).handle), req.handle);
        write_volatile(addr_of_mut!((*d).path), req.path);
        write_volatile(addr_of_mut!((*d).hook), Some(link.hook));
        write_volatile(addr_of_mut!((*d).done), Some(link.done));
        let mut i = 0;
        while i < 2 * WINDOW_MAX {
            write_volatile(addr_of_mut!((*d).result[i]), 0);
            i += 1;
        }
        write_volatile(addr_of_mut!((*d).cursor), req.key.first);
    }
    // the worker may run the step as soon as the runtime has the job: the phase goes first
    cell.phase.publish(POSTED);
    // SAFETY: the core's job call; the cell is a static of the file that outlives the job
    let id = unsafe {
        core.job_post(
            0,
            Some(step),
            Some(link.done),
            cell as *const PeakJob as *mut u8,
        )
    };
    if id > 0 {
        w.state = State::Pending;
        w.job = id as u32;
        0
    } else {
        cell.phase.publish(IDLE);
        w.state = State::Idle;
        id
    }
}

/// The key logic of one call with a valid request: the columns when ready (copied to `out`), 0 while pending, or an error.
///
/// # Safety
/// `out` is writable for the columns of the request (4 bytes each).
unsafe fn lookup<C: CoreApi>(
    core: &C,
    cell: &PeakJob,
    w: &mut Window,
    req: &Req,
    link: Link,
    out: *mut u8,
) -> i32 {
    let key = req.key;
    let state = w.state;
    if state == State::Pending {
        let active = if w.has_next { w.next.key } else { w.cur };
        if !w.abandon && key == active {
            return 0;
        }
        if !w.has_next && !w.abandon {
            // SAFETY: the core's job call
            unsafe { core.job_cancel(0, w.job) };
        }
        w.next = *req;
        w.has_next = true;
        w.abandon = false;
        return 0;
    }
    if w.cur == key {
        if state == State::Ready {
            // in `Done` (or `Idle`) the cell is the UI task's
            core::ptr::copy_nonoverlapping(
                addr_of!((*cell.data.get()).result) as *const u8,
                out,
                key.columns as usize * 4,
            );
            return key.columns as i32;
        }
        if let State::Failed(e) = state {
            return e;
        }
    }
    start(core, cell, w, req, link)
}

/// The client's module went away: stop its job; no result is kept.
fn reset_window<C: CoreApi>(core: &C, w: &mut Window) {
    if w.state == State::Pending {
        // SAFETY: the core's job call
        unsafe { core.job_cancel(0, w.job) };
        w.has_next = false;
        w.abandon = true;
    } else {
        w.state = State::Idle;
    }
}

impl PeakWindows {
    pub(crate) const fn new() -> PeakWindows {
        PeakWindows {
            win: [WINDOW0; CLIENTS],
        }
    }

    /// Make the windows in `slot` (the initial state is not all-zero bytes: `State` is written one by one).
    pub(crate) fn init(slot: &mut MaybeUninit<PeakWindows>) {
        let p = slot.as_mut_ptr();
        let mut i = 0;
        while i < CLIENTS {
            // SAFETY: `p` is the slot; each element is written whole, without reading the old bytes.
            unsafe { addr_of_mut!((*p).win[i]).write(WINDOW0) };
            i += 1;
        }
    }

    /// Client teardown (`Samples::release`).
    pub(crate) fn reset<C: CoreApi>(&mut self, core: &C, client: usize) {
        if client < CLIENTS {
            reset_window(core, &mut self.win[client]);
        }
    }

    /// The end of a job (the `PeakDone` event, applied after the call it arrived in): a newer request waits in `next`: start it. Otherwise take the result.
    pub(crate) fn finish<C: CoreApi>(
        &mut self,
        core: &C,
        tracks: &TrackCore,
        cell: &PeakJob,
        hook: Option<WindowStepFn>,
    ) {
        // the phase first: only in `DONE` is the cell the UI task's
        if cell.phase.phase() != DONE {
            return;
        }
        // SAFETY: the job ended (`DONE`): the cell is the UI task's
        let (client, status, done) = unsafe {
            let d = cell.data.get();
            (
                read_volatile(addr_of!((*d).client)) as usize,
                read_volatile(addr_of!((*d).status)),
                read_volatile(addr_of!((*d).done)),
            )
        };
        if client >= CLIENTS {
            return;
        }
        let w = &mut self.win[client];
        if w.state != State::Pending {
            return;
        }
        if w.has_next {
            w.has_next = false;
            let req = w.next;
            if let (Some(hook), Some(done)) = (hook, done) {
                start(core, cell, w, &req, Link { hook, done });
            } else {
                w.state = State::Idle;
            }
            return;
        }
        if w.abandon {
            w.abandon = false;
            w.state = State::Idle;
            return;
        }
        w.state = if status == 0 {
            State::Ready
        } else if status == JOB_E_CANCELLED {
            State::Idle
        } else {
            State::Failed(status)
        };
        // the slot was loaded again meanwhile: the next call asks again
        if !alive(tracks, w.client, &w.cur) {
            w.state = State::Idle;
        }
    }

    /// `sample_pool_peaks_window`, UI task.
    ///
    /// # Safety
    /// `out` is null or writable for `capacity` bytes.
    #[allow(clippy::too_many_arguments)]
    pub(crate) unsafe fn window<C: CoreApi>(
        &mut self,
        core: &C,
        jobs: &PeakJobs,
        done: JobDoneFn,
        tracks: &Tracks,
        pool: &Pool,
        handle: u32,
        index: u32,
        token: u32,
        first: u32,
        frames: u32,
        columns: u32,
        out: *mut u8,
        capacity: u32,
    ) -> i32 {
        let c = match tracks.client(handle) {
            Ok(c) => c,
            Err(e) => return e,
        };
        if let Err(e) = check_args(out, frames, columns, capacity) {
            return e;
        }
        let hook = match pool.window_step() {
            Some(h) => h,
            None => return E_CONTRACT,
        };
        let h = match tracks.core().peak_handle(c, index as usize, token) {
            Ok(h) => h,
            Err(e) => return e,
        };
        if first as u64 + frames as u64 > h.frames as u64 {
            return E_RANGE;
        }
        let mut req = REQ0;
        req.key = Key {
            index,
            token,
            first,
            frames,
            columns,
        };
        req.handle = h;
        match tracks.core().path(c, index as usize) {
            Ok(p) => {
                core::ptr::copy_nonoverlapping(p.as_ptr(), req.path.as_mut_ptr(), p.len());
                req.path[p.len()] = 0;
            }
            Err(e) => return e,
        }
        let w = &mut self.win[c];
        w.client = c;
        lookup(core, &jobs.job[c], w, &req, Link { hook, done }, out)
    }

    /// Test support: put the window of `client` in the Pending state as a started job would.
    #[cfg(test)]
    pub(crate) fn set_pending(&mut self, client: usize) {
        self.win[client].state = State::Pending;
    }

    /// Test support: was the pending window of `client` stopped by a reset?
    #[cfg(test)]
    pub(crate) fn abandoned(&self, client: usize) -> bool {
        self.win[client].abandon
    }
}

#[cfg(test)]
mod tests;
