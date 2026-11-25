use crate::*;
use std::ffi::c_int;

pub const CAP_MENU: c_int = 1;

pub const CAP_TEXT_PAGE: c_int = 2;

pub const CAP_LIST: c_int = 3;

pub const CAP_TOGGLE: c_int = 4;

pub const CAP_VALUE: c_int = 5;

pub const CAP_CONFIRM: c_int = 6;

pub const CAP_CHOICE: c_int = 7;

pub const CAP_TOAST: c_int = 8;

pub const CAP_TICK: c_int = 9;

/// Switches a capability off or on for the rest of the session.
pub fn set_cap(cap: c_int, on: bool) {
    with(|m| {
        if on {
            m.caps_off.remove(&cap);
        } else {
            m.caps_off.insert(cap);
        }
    });
}

/// True when the capability is on (the model only: the real service has no such call).
pub fn cap_on(cap: c_int) -> bool {
    with(|s| s.has(cap))
}
