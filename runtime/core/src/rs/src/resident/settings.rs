//! `SettingsFns`: the settings broker.
use crate::settings;
use core::slice::{from_raw_parts, from_raw_parts_mut};
use dkrt_abi::resident::SettingsFns;

unsafe extern "C" fn get(key: *const u8, key_len: u32, out: *mut u8, cap: u32) -> i32 {
    settings::get(from_raw_parts(key, key_len as usize), from_raw_parts_mut(out, cap as usize))
}
unsafe extern "C" fn put(key: *const u8, key_len: u32, value: *const u8, value_len: u32) -> i32 {
    settings::put(from_raw_parts(key, key_len as usize), from_raw_parts(value, value_len as usize))
}
unsafe extern "C" fn module_client() -> i32 {
    settings::module_client()
}
unsafe extern "C" fn module_get(client: u32, key: *const u8, out: *mut u8, cap: u32) -> i32 {
    settings::module_get(client, key, out, cap)
}
unsafe extern "C" fn module_put(client: u32, key: *const u8, value: *const u8, len: u32) -> i32 {
    settings::module_put(client, key, value, len)
}
unsafe extern "C" fn module_remove(client: u32, key: *const u8) -> i32 {
    settings::module_remove(client, key)
}
unsafe extern "C" fn capacity(out: *mut u32) {
    let (limit, used) = settings::capacity();
    *out = limit;
    *out.add(1) = used;
}

pub(super) static FNS: SettingsFns = SettingsFns { get, put, module_client, module_get, module_put, module_remove, capacity };
