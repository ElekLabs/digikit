//! The route options the menu offers for a row.

use super::*;
use crate::fxpage;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp;
use dkrt_sdk::*;

/// Operation 1 of the FX resident query facet. Row bits refer to DkrtFx.row.
pub const ROUTE_CHOICES_V1: u32 = 1;

#[repr(C)]
pub struct RouteChoicesV1 {
    pub row: u32,
    pub pending_sends: u32,
}

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

/// The route options the menu can offer for this row, given the edits the caller has pending.
pub unsafe fn route_choices<C: CoreApi>(core: &C, request: RouteChoicesV1) -> Result<u32, i32> {
    let RouteChoicesV1 { row, pending_sends } = request;
    if row >= dsp::MAX_FX as u32 || pending_sends >> dsp::MAX_FX != 0 {
        return Err(E_CONTRACT);
    }
    if fxpage::page_owner(row as usize) == 0 {
        return Err(E_NOT_FOUND);
    }
    Ok(allowed_choices(
        ss().allow[row as usize],
        row,
        pending_sends,
        slots(core),
    ))
}
