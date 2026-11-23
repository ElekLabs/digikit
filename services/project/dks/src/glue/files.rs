//! `DkFiles`: the engine's `Files` over the SDK's file calls, and the write job's entries (I/O task and UI task).
use super::c_strings::cpath;
use super::sdk_imports as sys;
use super::sdk_imports::{REPLACE, STORE};
use super::service_state::{FILES, PROJECT};
use core::ptr::{addr_of, addr_of_mut, read_volatile};
use dkrt_svc_project::engine::Files;

pub struct DkFiles;

impl Files for DkFiles {
    fn create(&mut self, path: &[u8], size: u32) -> i32 {
        let mut c = [0u8; 64];
        // SAFETY: the handle was opened in `init`; the call runs in the I/O step.
        unsafe {
            sys::dk_files_create(
                read_volatile(addr_of!(FILES)),
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
        unsafe {
            sys::dk_files_write(
                read_volatile(addr_of!(FILES)),
                h,
                0,
                buf.as_ptr(),
                buf.len() as u32,
            )
        }
    }
    fn close(&mut self, h: i32) -> i32 {
        // SAFETY: as above.
        unsafe { sys::dk_files_close(read_volatile(addr_of!(FILES)), h, core::ptr::null_mut()) }
    }
    fn abort(&mut self, h: i32) {
        // SAFETY: as above.
        unsafe {
            sys::dk_files_abort(read_volatile(addr_of!(FILES)), h);
        }
    }
    fn delete(&mut self, path: &[u8]) -> i32 {
        let mut c = [0u8; 64];
        // SAFETY: as above.
        unsafe { sys::dk_files_delete(read_volatile(addr_of!(FILES)), STORE, cpath(path, &mut c)) }
    }
}

/// The write job's step, in the I/O task (`dk_io_post`).
pub(super) unsafe extern "C" fn io_step(_arg: *mut u8) -> i32 {
    (*addr_of_mut!(PROJECT)).io_step(&mut DkFiles)
}

/// The end of the write job, in the UI task.
pub(super) unsafe extern "C" fn io_done(_arg: *mut u8, status: i32) {
    (*addr_of_mut!(PROJECT)).io_done(status)
}
