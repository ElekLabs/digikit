//! The tick of the file.
use crate::hooks::DKRT_PROJECT_PENDING;
use core::ptr::addr_of;

/// The tick of the file (the UI task, once per pass): what the hooks recorded, the work in progress, the next piece. Before `init` and after `fini` there is no owner and nothing to do.
pub unsafe extern "C" fn tick(_ctx: *mut u8) {
    crate::DK_SVC_OWNER.with(|p| {
        p.tick(&*addr_of!(DKRT_PROJECT_PENDING), &super::DK_JOB);
        super::exports::publish(p);
    });
}
