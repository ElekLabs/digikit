//! `DkHost`: the host state of the owner (`dkrt_svc_project::project::Host`): the handles `open` makes, the pass count and the listing in progress. It is a field of the owner
//! in the file's cell, not a static. All-zero bytes are its initial state.
use super::files::{set_client, DK_JOB};
use super::scope::SCOPE;
use super::sdk_imports as sys;
use crate::hooks::DKRT_PROJECT_PENDING;
use dkrt_abi::provider::Status;
use dkrt_abi::service::SettingsOverlay;
use dkrt_svc_project::engine::{Pending, StoreJob};
use dkrt_svc_project::project::{Host, Project};

/// The owner object of the service file: the engine over this file's host state. The file's cell holds the one instance (`DK_SVC_OWNER`).
pub type Owner = Project<DkHost>;

#[repr(C, align(4))]
pub(super) struct Entry(pub(super) [u8; 304]);

pub struct DkHost {
    /// The listing client of this file, opened in `init`.
    pub(super) files: *mut u8,
    /// The handle of core/ui: null when the image has no core/ui (the warnings are then in the log only).
    pub(super) ui: *mut u8,
    /// UI passes since `init`: the clock when the image has no block counter.
    pub(super) passes: u32,
    /// The listing in progress: its ticket, and the entry the poll fills.
    pub(super) ticket: u32,
    pub(super) entry: Entry,
}

impl DkHost {
    /// UI passes since `init`.
    pub fn passes(&self) -> u32 {
        self.passes
    }
}

// SAFETY: integers, null pointers and a byte array: all-zero bytes are the initial state.
unsafe impl Host for DkHost {
    /// The listing client that this file's `init` is the only place to open, and core/ui (optional: without it the result is a failure and the handle stays null).
    fn open(&mut self) -> Result<(), Status> {
        super::exports::reset_status();
        let ctx = dkrt_svc_rt::init_ctx();
        let mut f: *mut u8 = core::ptr::null_mut();
        // SAFETY: `ctx` is the module context of this `init`.
        let r = unsafe { sys::dk_files_open(ctx, &mut f) };
        if r != 0 {
            return Err(r);
        }
        self.files = f;
        set_client(f);
        let mut ui: *mut u8 = core::ptr::null_mut();
        // SAFETY: as above.
        if unsafe { sys::dk_ui_open(ctx, &mut ui) } == 0 {
            self.ui = ui;
        }
        Ok(())
    }

    fn begin_pass(&mut self) {
        self.passes = self.passes.wrapping_add(1);
    }

    fn hook_record(&self) -> &'static Pending {
        // SAFETY: the hooks' record is a static of this file; it is plain words that the hooks write (see `Pending`).
        unsafe { &*core::ptr::addr_of!(DKRT_PROJECT_PENDING) }
    }

    fn job(&self) -> &'static StoreJob {
        &DK_JOB
    }

    fn overlay() -> *const SettingsOverlay {
        &SCOPE
    }
}
