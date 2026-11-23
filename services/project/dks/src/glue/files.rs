//! `DkFiles`: the engine's `Files` over the SDK's file calls, and the write job's entries (I/O task and UI task).
use super::c_strings::cpath;
use super::host::Owner;
use super::sdk_imports as sys;
use super::sdk_imports::{REPLACE, STORE};
use dkrt_svc_project::engine::Files;

/// The file calls of the write job: the listing client's handle, which the I/O task reads.
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

/// The write job's step, in the I/O task (`dk_io_post`). It reaches the owner by its pointer, not through `with`: the UI task may be inside the owner (`tick`) while the I/O
/// task runs the step. The step forms references to the writer, the file buffer and the file client only (`Project::io_step`), never to the whole owner; the UI task leaves
/// those alone while a job runs.
pub unsafe extern "C" fn io_step(_arg: *mut u8) -> i32 {
    let project = crate::DK_SVC_OWNER.instance();
    if project.is_null() {
        return dkrt_sdk::E_NOT_FOUND;
    }
    Owner::io_step(project)
}

/// The end of the write job, in the UI task.
pub unsafe extern "C" fn io_done(_arg: *mut u8, status: i32) {
    let project = crate::DK_SVC_OWNER.instance();
    if !project.is_null() {
        (*project).io_done(status)
    }
}
