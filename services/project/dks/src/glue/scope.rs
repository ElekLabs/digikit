//! The project scope table that the runtime's settings overlay forwards to.
use crate::DK_SVC_OWNER;
use dkrt_abi::service::{SettingsOverlay, SETTINGS_OVERLAY_VERSION};
use dkrt_sdk::proto::{E_SET_NOKEY, E_SET_OFF};

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
    // no owner (before `init` or after `fini`): no value
    DK_SVC_OWNER
        .with(|p| p.scope_get(bytes(id, id_len), bytes(key, key_len), o))
        .unwrap_or(E_SET_NOKEY)
}

unsafe extern "C" fn scope_put(
    id: *const u8,
    id_len: u32,
    key: *const u8,
    key_len: u32,
    value: *const u8,
    len: u32,
) -> i32 {
    // no owner: the layer has nothing to hold, the record alone takes the change
    DK_SVC_OWNER
        .with(|p| p.scope_put(bytes(id, id_len), bytes(key, key_len), bytes(value, len)))
        .unwrap_or(E_SET_OFF)
}

unsafe extern "C" fn scope_remove(id: *const u8, id_len: u32, key: *const u8, key_len: u32) -> i32 {
    DK_SVC_OWNER
        .with(|p| p.scope_remove(bytes(id, id_len), bytes(key, key_len)))
        .unwrap_or(E_SET_OFF)
}

/// The table `dkc_settings_overlay_set` hands to the runtime.
pub static SCOPE: SettingsOverlay = SettingsOverlay {
    version: SETTINGS_OVERLAY_VERSION,
    get: scope_get,
    put: scope_put,
    remove: scope_remove,
};
