//! The machines service as the host tests of the core's cleanup and invariants see it: its voice rows are its own static (`DKRT_VROWS`), not the loader's state. A host
//! test seeds and reads them here (the core names no service crate outside `handlers`). Take the test lock first (`test_lock::LOCK`): the rows are a static.

/// The machines service's voice rows.
pub(crate) fn vrows() -> &'static mut dkrt_svc_machines::rows::VoiceRows {
    dkrt_svc_machines::rows::vrows()
}

/// The builder call `dk_module_ctx_add_machine` of the module whose context is `ctx` (its `init` is open), for the machine of `index` of its header table.
pub(crate) unsafe fn add_machine(ctx: *mut u8, index: u32) {
    dkrt_svc_machines::build::add_machine(&crate::package::core_api::RealCore, ctx, index, b"Test\0".as_ptr(), b"TST\0".as_ptr());
}
