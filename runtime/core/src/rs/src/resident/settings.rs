//! `SettingsFns`: the settings broker. The module client entries are the owner's own `extern "C"` functions.
use crate::settings;
use core::slice::{from_raw_parts, from_raw_parts_mut};
use dkrt_abi::resident::{Group, SettingsFns};

unsafe extern "C" fn get(key: *const u8, key_len: u32, out: *mut u8, cap: u32) -> i32 {
    settings::get(from_raw_parts(key, key_len as usize), from_raw_parts_mut(out, cap as usize))
}
unsafe extern "C" fn put(key: *const u8, key_len: u32, value: *const u8, value_len: u32) -> i32 {
    settings::put(from_raw_parts(key, key_len as usize), from_raw_parts(value, value_len as usize))
}
unsafe extern "C" fn capacity(out: *mut u32) {
    let (limit, used) = settings::capacity();
    *out = limit;
    *out.add(1) = used;
}

pub(super) static FNS: Group<SettingsFns> = Group::new(SettingsFns {
    get,
    put,
    module_client: settings::module_client,
    module_get: settings::module_get,
    module_put: settings::module_put,
    module_remove: settings::module_remove,
    capacity,
});
