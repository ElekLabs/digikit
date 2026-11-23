//! The project scope table that the runtime's settings overlay forwards to.
use super::service_state::PROJECT;
use core::ptr::{addr_of, addr_of_mut};
use dkrt_abi::service::{SettingsOverlay, SETTINGS_OVERLAY_VERSION};

unsafe fn bytes<'a>(p: *const u8, n: u32) -> &'a [u8] {
    if n == 0 {
        &[]
    } else {
        core::slice::from_raw_parts(p, n as usize)
    }
}

unsafe extern "C" fn scope_get(
    id: *const u8,
    id_len: u32,
    key: *const u8,
    key_len: u32,
    out: *mut u8,
    cap: u32,
) -> i32 {
    let o: &mut [u8] = if cap == 0 {
        &mut []
    } else {
        core::slice::from_raw_parts_mut(out, cap as usize)
    };
    (*addr_of!(PROJECT)).scope_get(bytes(id, id_len), bytes(key, key_len), o)
}

unsafe extern "C" fn scope_put(
    id: *const u8,
    id_len: u32,
    key: *const u8,
    key_len: u32,
    value: *const u8,
    len: u32,
) -> i32 {
    (*addr_of_mut!(PROJECT)).scope_put(bytes(id, id_len), bytes(key, key_len), bytes(value, len))
}

unsafe extern "C" fn scope_remove(id: *const u8, id_len: u32, key: *const u8, key_len: u32) -> i32 {
    (*addr_of_mut!(PROJECT)).scope_remove(bytes(id, id_len), bytes(key, key_len))
}

/// The table `dkc_settings_overlay_set` hands to the runtime.
pub static SCOPE: SettingsOverlay = SettingsOverlay {
    version: SETTINGS_OVERLAY_VERSION,
    get: scope_get,
    put: scope_put,
    remove: scope_remove,
};
