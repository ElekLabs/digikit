//! Sample part of the DSP loader: the serialized window-zero transfer for a sample job.
use crate::dsp::*;

/// Serialized window-zero transfer for an owned sample job.
pub unsafe fn sample_send(dst: u32, src: *const u8, len: u32) -> i32 {
    call_send(dst, src, len)
}

pub unsafe fn sample_poll() -> Option<bool> {
    poll_done()
}

pub unsafe fn sample_idle() -> bool {
    ds().phase == PH_IDLE && ds().nwait == 0
}
