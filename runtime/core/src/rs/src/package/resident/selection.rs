//! The selection record on the card.
use super::table;
use dkrt_sdk::sel::MAP_N;

/// Add a store path to the saved selection.
#[inline(always)]
pub(crate) unsafe fn selection_add(path: &[u8]) -> i32 {
    (table().selection.add)(path.as_ptr(), path.len() as u32)
}

/// The legacy state words of the record this boot started from.
#[inline(always)]
pub(crate) unsafe fn legacy_state() -> [u32; MAP_N] {
    let mut out = [0u32; MAP_N];
    (table().selection.legacy_state)(out.as_mut_ptr());
    out
}

/// Write the record with the legacy state cleared.
#[inline(always)]
pub(crate) unsafe fn legacy_state_clear() -> i32 {
    (table().selection.legacy_state_clear)()
}
