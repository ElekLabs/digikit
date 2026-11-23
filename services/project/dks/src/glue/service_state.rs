//! The state of the file: the one `ProjectService`, the handles `setup` opened, the pass count and the listing in progress. All state is static: one service, one file.
use dkrt_svc_project::engine::ProjectService;

pub(super) static mut PROJECT: ProjectService = ProjectService::new();
/// The handle `setup` opened: the listing client of this file.
pub(super) static mut FILES: *mut u8 = core::ptr::null_mut();
/// The handle of core/ui that `setup` opened: null when the image has no core/ui (the warnings are then in the log only).
pub(super) static mut UI: *mut u8 = core::ptr::null_mut();
/// UI passes since `init`: the clock when the image has no block counter.
pub(super) static mut PASSES: u32 = 0;
/// The listing in progress: its ticket, and the entry the poll fills.
pub(super) static mut TICKET: u32 = 0;
#[repr(C, align(4))]
pub(super) struct Entry(pub(super) [u8; 304]);
pub(super) static mut ENTRY: Entry = Entry([0; 304]);
