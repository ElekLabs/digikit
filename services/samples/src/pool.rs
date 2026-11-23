//! The sample backing plug-in point (`dkrt_pool.h`, version 1) as the service uses it: a table of function pointers, found through the adapter's data table
//! `sample_pool_info` (`CoreApi::info`), not through link symbols of the image. A service file links no adapter symbol; the table is the one place that names
//! the adapter, and `Provider::start` of the owner (or a host test, with `Pool::new`) installs it.
//!
//! `sample_pool_info` (big-endian words, ColdFire): magic `DKSP`, version, a reserved word, then the addresses of `dkpool_load`, `dkpool_release`, `dkpool_check`,
//! `dkpool_free_frames`, `dkpool_tick` and the optional `dkpool_cancel`, `dkpool_peaks`, `dkpool_window_step` (0: the adapter has no such hook).
use dkrt_abi::core_api::CoreApi;
use dkrt_sdk::sample_tracks::{FileId, Handle};
use dkrt_sdk::E_CONTRACT;

pub type LoadFn = unsafe extern "C" fn(u32, *const FileId, *const u8, *mut Handle) -> i32;
pub type ReleaseFn = unsafe extern "C" fn(*const Handle);
pub type CheckFn = unsafe extern "C" fn(*const Handle) -> i32;
pub type FreeFramesFn = unsafe extern "C" fn() -> u32;
pub type TickFn = unsafe extern "C" fn();
pub type CancelFn = unsafe extern "C" fn(u32);
pub type PeaksFn = unsafe extern "C" fn(*const Handle, *mut u8, u32) -> i32;
pub type WindowStepFn =
    unsafe extern "C" fn(*const Handle, *const u8, u32, u32, u32, *mut i16, *mut u32) -> i32;

/// The calls of one adapter.
#[derive(Clone, Copy)]
pub struct PoolFns {
    pub load: LoadFn,
    pub release: ReleaseFn,
    pub check: CheckFn,
    pub free_frames: FreeFramesFn,
    pub tick: TickFn,
    pub cancel: Option<CancelFn>,
    pub peaks: Option<PeaksFn>,
    pub window_step: Option<WindowStepFn>,
}

/// The magic word of `sample_pool_info` (`DKSP`) and the plug-in point version the service speaks.
pub const INFO_MAGIC: u32 = 0x444b_5350;
pub const VERSION: u32 = 1;
/// The name of the table in the image's info table.
pub const INFO_NAME: &[u8] = b"sample_pool_info";

/// The adapter of the pool: its calls, or none (no backing). The owner holds one, found at `start` from `sample_pool_info`; a host test gives its own.
#[derive(Clone, Copy)]
pub struct Pool {
    fns: Option<PoolFns>,
}

impl Pool {
    pub const fn new(fns: Option<PoolFns>) -> Pool {
        Pool { fns }
    }

    /// No backing.
    pub const fn none() -> Pool {
        Pool { fns: None }
    }

    /// The adapter that `sample_pool_info` of the image describes (`from_info`).
    ///
    /// # Safety
    /// The table of the image, when present, holds the addresses of the adapter's functions.
    pub unsafe fn from_info<C: CoreApi>(core: &C) -> Pool {
        Pool {
            fns: from_info(core),
        }
    }

    /// Is a backing adapter of the right version installed?
    pub fn present(&self) -> bool {
        self.fns.is_some()
    }

    pub(crate) unsafe fn load(
        &self,
        req: u32,
        id: *const FileId,
        path: *const u8,
        out: *mut Handle,
    ) -> i32 {
        match self.fns {
            Some(f) => (f.load)(req, id, path, out),
            None => E_CONTRACT,
        }
    }

    pub(crate) unsafe fn release(&self, h: *const Handle) {
        if let Some(f) = self.fns {
            (f.release)(h)
        }
    }

    pub(crate) unsafe fn check(&self, h: *const Handle) -> i32 {
        match self.fns {
            Some(f) => (f.check)(h),
            None => E_CONTRACT,
        }
    }

    pub(crate) unsafe fn free_frames(&self) -> u32 {
        match self.fns {
            Some(f) => (f.free_frames)(),
            None => 0,
        }
    }

    pub(crate) unsafe fn tick(&self) {
        if let Some(f) = self.fns {
            (f.tick)()
        }
    }

    pub(crate) unsafe fn cancel(&self, req: u32) {
        if let Some(c) = self.fns.and_then(|f| f.cancel) {
            c(req)
        }
    }

    pub(crate) fn peaks_present(&self) -> bool {
        self.fns.is_some_and(|f| f.peaks.is_some())
    }

    pub(crate) unsafe fn peaks(&self, h: *const Handle, out: *mut u8, capacity: u32) -> i32 {
        match self.fns.and_then(|f| f.peaks) {
            Some(p) => p(h, out, capacity),
            None => E_CONTRACT,
        }
    }

    /// The optional `dkpool_window_step` hook of the adapter.
    pub(crate) fn window_step(&self) -> Option<WindowStepFn> {
        self.fns.and_then(|f| f.window_step)
    }
}

/// The calls of the adapter that `sample_pool_info` of the image describes: `None` without the table, with another magic, version or device. A function word is a
/// 32-bit ColdFire address: on a host with wider pointers there is no way to call it, so the answer is `None` there (a host test installs its own with `set`).
unsafe fn from_info<C: CoreApi>(core: &C) -> Option<PoolFns> {
    #[cfg(not(target_pointer_width = "32"))]
    {
        let _ = core;
        None
    }
    #[cfg(target_pointer_width = "32")]
    {
        let t = core.info(dkneeds::keys::name_hash_bytes(INFO_NAME));
        if t.is_null() {
            return None;
        }
        let w = |i: usize| core::ptr::read_volatile(t.add(i));
        if w(0) != INFO_MAGIC || w(1) != VERSION {
            return None;
        }
        let opt = |v: u32| if v == 0 { None } else { Some(v as usize) };
        if w(3) == 0 || w(4) == 0 || w(5) == 0 || w(6) == 0 || w(7) == 0 {
            return None;
        }
        Some(PoolFns {
            load: core::mem::transmute::<usize, LoadFn>(w(3) as usize),
            release: core::mem::transmute::<usize, ReleaseFn>(w(4) as usize),
            check: core::mem::transmute::<usize, CheckFn>(w(5) as usize),
            free_frames: core::mem::transmute::<usize, FreeFramesFn>(w(6) as usize),
            tick: core::mem::transmute::<usize, TickFn>(w(7) as usize),
            cancel: opt(w(8)).map(|a| core::mem::transmute::<usize, CancelFn>(a)),
            peaks: opt(w(9)).map(|a| core::mem::transmute::<usize, PeaksFn>(a)),
            window_step: opt(w(10)).map(|a| core::mem::transmute::<usize, WindowStepFn>(a)),
        })
    }
}

// ---- a backing for host tests ---------------------------------------------------------------------------------------------------

unsafe extern "C" fn refuse_load(_: u32, _: *const FileId, _: *const u8, _: *mut Handle) -> i32 {
    E_CONTRACT
}
unsafe extern "C" fn nothing_to_release(_: *const Handle) {}
unsafe extern "C" fn refuse_check(_: *const Handle) -> i32 {
    -15
}
unsafe extern "C" fn no_free_frames() -> u32 {
    0
}
unsafe extern "C" fn nothing_to_tick() {}

/// A backing that refuses every load and has nothing pending, for the host tests of the service and of the service file's assembly (`Pool::new(Some(refusing()))`).
pub fn refusing() -> PoolFns {
    PoolFns {
        load: refuse_load,
        release: nothing_to_release,
        check: refuse_check,
        free_frames: no_free_frames,
        tick: nothing_to_tick,
        cancel: None,
        peaks: None,
        window_step: None,
    }
}
