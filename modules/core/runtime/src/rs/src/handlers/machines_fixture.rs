//! The machines service as the host tests of the core's cleanup and invariants see it: its voice rows are its own static (`DKRT_VROWS`), not the loader's state. A host
//! test seeds and reads them here (the core names no service crate outside `handlers`). Take the test lock first (`test_lock::LOCK`): the rows are a static.

/// The machines service's voice rows.
pub(crate) fn vrows() -> &'static mut dkrt_svc_machines::rows::VoiceRows {
    dkrt_svc_machines::rows::vrows()
}
