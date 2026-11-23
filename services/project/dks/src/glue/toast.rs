//! Toasts: one short line through core/ui.
use super::sdk_imports as sys;
use super::service_state::UI;
use core::ptr::{addr_of, read_volatile};

/// Characters a toast shows (`DK_UI_TOAST` of <dk/platform/ui.h>, the terminator included).
pub(super) const TOAST_MAX: usize = 32;

/// A toast of the parts joined, upper case, cut to what a toast shows. Nothing when the image has no core/ui.
pub(super) fn show(parts: &[&[u8]]) {
    // SAFETY: the handle is set in `setup` and read in the UI task only.
    let ui = unsafe { read_volatile(addr_of!(UI)) };
    if ui.is_null() {
        return;
    }
    let mut text = [0u8; TOAST_MAX];
    let mut n = 0;
    for part in parts {
        for &b in part.iter() {
            if n < TOAST_MAX - 1 {
                text[n] = b.to_ascii_uppercase();
                n += 1;
            }
        }
    }
    // SAFETY: `text` is NUL terminated; the result (0 shown, else a failure value) changes nothing for the service.
    unsafe {
        sys::dk_ui_toast(ui, text.as_ptr());
    }
}

/// `<name><suffix>`, with the name cut so that the suffix stays whole.
pub(super) fn show_name(name: &[u8], suffix: &[u8]) {
    let keep = name.len().min(TOAST_MAX - 1 - suffix.len());
    show(&[&name[..keep], suffix]);
}
