//! `SettingsFns`: the settings broker (`settings.rs`): the runtime's own keys and one client per module.

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SettingsFns {
    /// The value of the runtime key: its length, or an `E_*` code.
    pub get: unsafe extern "C" fn(key: *const u8, key_len: u32, out: *mut u8, cap: u32) -> i32,
    pub put:
        unsafe extern "C" fn(key: *const u8, key_len: u32, value: *const u8, value_len: u32) -> i32,
    pub module_client: unsafe extern "C" fn() -> i32,
    pub module_get:
        unsafe extern "C" fn(client: u32, key: *const u8, out: *mut u8, cap: u32) -> i32,
    pub module_put:
        unsafe extern "C" fn(client: u32, key: *const u8, value: *const u8, len: u32) -> i32,
    pub module_remove: unsafe extern "C" fn(client: u32, key: *const u8) -> i32,
    /// The record's area and what it uses now: `out[0]` and `out[1]`.
    pub capacity: unsafe extern "C" fn(out: *mut u32),
}
