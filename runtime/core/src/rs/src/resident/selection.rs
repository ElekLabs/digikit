//! `SelectionFns`: the selection record.
use crate::selection::{self, REC, REC_COPY, REC_VALID};
use core::ptr::addr_of;
use dkrt_abi::resident::{Group, SelectionFns};

unsafe extern "C" fn add(path: *const u8, len: u32) -> i32 {
    selection::selection_add(core::slice::from_raw_parts(path, len as usize))
}
unsafe extern "C" fn legacy_state(out: *mut u32) {
    let map = (*addr_of!(REC)).map;
    let mut i = 0;
    while i < map.len() {
        *out.add(i) = map[i];
        i += 1;
    }
}
unsafe extern "C" fn legacy_state_clear() -> i32 {
    let mut r = REC;
    r.map = [0; dkrt_sdk::sel::MAP_N];
    selection::update(r, REC_COPY, REC_VALID)
}

pub(super) static FNS: Group<SelectionFns> = Group::new(SelectionFns { add, legacy_state, legacy_state_clear });
