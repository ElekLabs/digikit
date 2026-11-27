//! `DkFiles`: the engine's `Files` over the SDK's file calls, and the write job's entries (I/O task and UI task).
use super::c_strings::cpath;
use super::sdk_imports as sys;
use super::sdk_imports::{REPLACE, STORE};
use core::cell::UnsafeCell;
use dkrt_svc_project::engine::{Files, StoreJob};

/// The file calls of the write job: the listing client's handle, which the I/O task reads from `CLIENT`.
#[derive(Clone, Copy)]
pub struct DkFiles {
    pub(super) handle: *mut u8,
}

impl Files for DkFiles {
    fn create(&mut self, path: &[u8], size: u32) -> i32 {
        let mut c = [0u8; 64];
        // SAFETY: the handle was opened in `init`; the call runs in the I/O step.
        unsafe {
            sys::dk_files_create(
                self.handle,
                STORE,
                cpath(path, &mut c),
                size,
                core::ptr::null(),
                REPLACE,
            )
        }
    }
    fn write(&mut self, h: i32, buf: &[u8]) -> i32 {
        // SAFETY: as above; the image is 16-byte aligned.
        unsafe { sys::dk_files_write(self.handle, h, 0, buf.as_ptr(), buf.len() as u32) }
    }
    fn close(&mut self, h: i32) -> i32 {
        // SAFETY: as above.
        unsafe { sys::dk_files_close(self.handle, h, core::ptr::null_mut()) }
    }
    fn abort(&mut self, h: i32) {
        // SAFETY: as above.
        unsafe {
            sys::dk_files_abort(self.handle, h);
        }
    }
    fn delete(&mut self, path: &[u8]) -> i32 {
        let mut c = [0u8; 64];
        // SAFETY: as above.
        unsafe { sys::dk_files_delete(self.handle, STORE, cpath(path, &mut c)) }
    }
}

/// The write job's cell: its data is the I/O task's while a job runs, so it is a static of this file and not a field of the owner (`StoreJob`: the phase word hands it between the
/// tasks). The tick passes it to the engine; the two entries below are its I/O-task and UI-task ends.
pub static DK_JOB: StoreJob = StoreJob::new();

/// The listing client's handle for the I/O task, set once in `init` (`DkHost::open`) before any job is posted; the owner keeps its own copy for the UI task.
struct Client(UnsafeCell<*mut u8>);

// SAFETY: one aligned word, written before the first job is posted and read by the I/O task only while a job runs
unsafe impl Sync for Client {}

static CLIENT: Client = Client(UnsafeCell::new(core::ptr::null_mut()));

/// Keep the handle the I/O task's file calls use (`DkHost::open`).
pub(super) fn set_client(handle: *mut u8) {
    // SAFETY: see `Client`
    unsafe { core::ptr::write_volatile(CLIENT.0.get(), handle) }
}

/// The write job's step, in the I/O task (`dk_io_post`). It reaches the job's cell and the client handle, both statics of this file, and never the owner: the UI task may be inside
/// the owner (`tick`) while the I/O task runs the step.
pub unsafe extern "C" fn io_step(_arg: *mut u8) -> i32 {
    // SAFETY: see `Client`
    let handle = core::ptr::read_volatile(CLIENT.0.get());
    DK_JOB.io_step(&mut DkFiles { handle })
}

/// The end of the write job, in the UI task: the cell takes the status and the UI task owns its data again.
pub unsafe extern "C" fn io_done(_arg: *mut u8, status: i32) {
    DK_JOB.done(status)
}
