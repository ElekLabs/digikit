//! Peak windows for `sample_pool_peaks_window`: min/max of `columns` equal parts of a frame range of a READY pool slot. The PCM is
//! read in the worker task (`worker.rs`) through the optional adapter hook `dkpool_window_step` (dkrt_pool.h); the UI task only
//! starts, polls and copies.
//!
//! One window per sample-tracks client. A request is a key (slot index, token, first frame, frame count, columns). States:
//!   Idle    nothing requested, or the last result is not valid any more
//!   Pending a job runs for `cur`; `next` holds a newer request when `has_next`
//!   Ready   `result` holds the pairs of `cur`
//!   Failed  the job ended with an error; it is returned until the key changes (no retry)
//!
//! Task rules. While the state is Pending the worker task may read and write `cur.handle`, `cur.path`, `cur.key`, `result` and
//! `cursor`. The UI task does not touch these then. A new key meanwhile goes to `next`; the old job is cancelled and `done`
//! (UI task) starts the job for `next` after the old one has ended. So two jobs never share a window. A window has at most one job,
//! so `done` needs no job id.
use crate::pool::{Pool, WindowStepFn};
use crate::tracks::{TrackCore, Tracks, CLIENTS};
use core::mem::MaybeUninit;
use core::ptr::{addr_of, addr_of_mut, write_volatile};
use dkrt_abi::core_api::{CoreApi, JOB_E_CANCELLED};
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

const REQ0: Req = Req {
    key: Key {
        index: 0,
        token: 0,
        first: 0,
        frames: 0,
        columns: 0,
    },
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

pub(crate) struct Window {
    client: usize,
    state: State,
    job: u32,
    cur: Req,
    next: Req,
    has_next: bool,
    /// The client went away while Pending: `done` ends in Idle.
    abandon: bool,
    /// `columns` pairs of int16 min, max; written by the worker.
    result: [i16; 2 * WINDOW_MAX],
    /// Next frame to read; written by the worker.
    cursor: u32,
    /// The core that posted the job (a `*const C` of the `C` that `start` was called with): `done` runs without a core argument and needs it to start the next job.
    core: *const u8,
    /// The tracks that the key of a request refers to (the owner's, set by `window`): `done` asks them whether the slot still holds the same load.
    tracks: *const TrackCore,
    /// The adapter's `dkpool_window_step` hook (set by `window`): the worker task calls it.
    hook: Option<WindowStepFn>,
}

const WINDOW0: Window = Window {
    client: 0,
    state: State::Idle,
    job: 0,
    cur: REQ0,
    next: REQ0,
    has_next: false,
    abandon: false,
    result: [0; 2 * WINDOW_MAX],
    cursor: 0,
    core: core::ptr::null(),
    tracks: core::ptr::null(),
    hook: None,
};

/// The peak windows of the owner, one per client. A posted job holds a pointer to its window, so the owner does not move while a job is pending (it lives in the file's cell).
pub(crate) struct PeakWindows {
    win: [Window; CLIENTS],
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
unsafe fn alive(w: *const Window, key: &Key) -> bool {
    !(*w).tracks.is_null()
        && (*(*w).tracks)
            .peak_handle((*w).client, key.index as usize, key.token)
            .is_ok()
}

/// Zero the result, set the cursor and post the job for `cur`. 0, or the error of `worker::post` (the state is Idle then).
unsafe fn start<C: CoreApi>(core: &C, w: *mut Window) -> i32 {
    let mut i = 0;
    while i < 2 * WINDOW_MAX {
        write_volatile(addr_of_mut!((*w).result[i]), 0);
        i += 1;
    }
    write_volatile(addr_of_mut!((*w).cursor), (*w).cur.key.first);
    (*w).core = core as *const C as *const u8;
    let id = core.job_post(0, Some(step), Some(done::<C>), w as *mut u8);
    if id > 0 {
        (*w).state = State::Pending;
        (*w).job = id as u32;
        0
    } else {
        (*w).state = State::Idle;
        id
    }
}

/// Worker task. Calls the adapter for one chunk and returns its value (1: more). It touches only the fields that belong to the
/// worker while the state is Pending.
unsafe extern "C" fn step(ctx: *mut u8) -> i32 {
    let w = ctx as *mut Window;
    match (*w).hook {
        Some(hook) => hook(
            addr_of!((*w).cur.handle),
            addr_of!((*w).cur.path) as *const u8,
            (*w).cur.key.first,
            (*w).cur.key.frames,
            (*w).cur.key.columns,
            addr_of_mut!((*w).result) as *mut i16,
            addr_of_mut!((*w).cursor),
        ),
        None => E_CONTRACT,
    }
}

/// UI task, once per job. A newer request waits in `next`: start it. Otherwise take the result.
unsafe extern "C" fn done<C: CoreApi>(ctx: *mut u8, status: i32) {
    let w = ctx as *mut Window;
    let core = &*((*w).core as *const C);
    if (*w).state != State::Pending {
        return;
    }
    if (*w).has_next {
        (*w).cur = (*w).next;
        (*w).has_next = false;
        start(core, w);
        return;
    }
    if (*w).abandon {
        (*w).abandon = false;
        (*w).state = State::Idle;
        return;
    }
    (*w).state = if status == 0 {
        State::Ready
    } else if status == JOB_E_CANCELLED {
        State::Idle
    } else {
        State::Failed(status)
    };
    // the slot was loaded again meanwhile: the next call asks again
    let key = (*w).cur.key;
    if !alive(w, &key) {
        (*w).state = State::Idle;
    }
}

/// The key logic of one call with a valid request: the columns when ready (copied to `out`), 0 while pending, or an error.
unsafe fn lookup<C: CoreApi>(core: &C, w: *mut Window, req: &Req, out: *mut u8) -> i32 {
    let key = req.key;
    let state = (*w).state;
    if state == State::Pending {
        let active = if (*w).has_next {
            (*w).next.key
        } else {
            (*w).cur.key
        };
        if !(*w).abandon && key == active {
            return 0;
        }
        if !(*w).has_next && !(*w).abandon {
            core.job_cancel(0, (*w).job);
        }
        (*w).next = *req;
        (*w).has_next = true;
        (*w).abandon = false;
        return 0;
    }
    if (*w).cur.key == key {
        if state == State::Ready {
            core::ptr::copy_nonoverlapping(
                addr_of!((*w).result) as *const u8,
                out,
                key.columns as usize * 4,
            );
            return key.columns as i32;
        }
        if let State::Failed(e) = state {
            return e;
        }
    }
    (*w).cur = *req;
    start(core, w)
}

/// The client's module went away: stop its job; no result is kept.
unsafe fn reset_window<C: CoreApi>(core: &C, w: *mut Window) {
    if (*w).state == State::Pending {
        core.job_cancel(0, (*w).job);
        (*w).has_next = false;
        (*w).abandon = true;
    } else {
        (*w).state = State::Idle;
    }
}

impl PeakWindows {
    pub(crate) const fn new() -> PeakWindows {
        PeakWindows {
            win: [WINDOW0; CLIENTS],
        }
    }

    /// Make the windows in `slot` (the initial state is not all-zero bytes: `State` and the null pointers are written one by one).
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
            // SAFETY: the window is a field of `self`; a job that runs for it works on the same bytes by the task rules in the module doc
            unsafe { reset_window(core, addr_of_mut!(self.win[client])) };
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
        let w = addr_of_mut!(self.win[c]);
        (*w).client = c;
        (*w).tracks = tracks.core() as *const TrackCore;
        (*w).hook = Some(hook);
        lookup(core, w, &req, out)
    }
}

#[cfg(test)]
mod tests;
