//! The route options the menu offers for a row.

use super::*;
use dkrt_abi::dsp;

pub(super) fn allowed_choices(allowed: u8, row: u32, pending_sends: u32, slots: u32) -> u32 {
    let current = 1 << row;
    let keep_send = pending_sends & current != 0;
    let sends = (0..dsp::MAX_FX)
        .filter(|k| pending_sends & (1 << k) != 0)
        .count() as u32;
    let send_available = keep_send || sends < slots;
    u32::from(if send_available {
        allowed
    } else {
        allowed & !ROUTE_SEND
    })
}
