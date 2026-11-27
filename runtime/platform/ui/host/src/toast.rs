use crate::*;
use std::ffi::{c_char, c_int, c_void};

#[no_mangle]
pub extern "C" fn dk_ui_toast(_ui: *mut c_void, text: *const c_char) -> c_int {
    with(|s| {
        if !s.has(CAP_TOAST) {
            return -1;
        }
        let t: String = text_cut(text, 4096).chars().take(DK_UI_TOAST).collect();
        s.log(format!("toast {t}"));
        s.toasts.push(t);
        0
    })
}

/// Toasts shown so far, oldest first.
pub fn toasts() -> Vec<String> {
    with(|m| m.toasts.clone())
}
