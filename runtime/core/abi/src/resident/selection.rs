//! `SelectionFns`: the selection record on the card (`selection.rs`).

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SelectionFns {
    /// Add a store path to the saved selection: 0 or an `E_*` code.
    pub add: unsafe extern "C" fn(path: *const u8, len: u32) -> i32,
    /// The legacy state words of the record this boot started from: `MAP_N` words in `out`.
    pub legacy_state: unsafe extern "C" fn(out: *mut u32),
    /// Write the record with the legacy state cleared: 0 or an `E_*` code.
    pub legacy_state_clear: unsafe extern "C" fn() -> i32,
}
