//! The exports of the file: `dk_project_current` and `dk_project_status`.
use super::service_state::PROJECT;
use core::ptr::addr_of;

/// `dk_project_current` (export of the file): the slot and the name of the current project, `E_NOT_FOUND` without one.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_project_current(slot: *mut u32, name: *mut u8) -> i32 {
    match (*addr_of!(PROJECT)).current() {
        Ok((s, n)) => {
            if !slot.is_null() {
                *slot = s;
            }
            if !name.is_null() {
                let mut i = 0;
                while i < 16 {
                    *name.add(i) = n[i];
                    i += 1;
                }
            }
            0
        }
        Err(e) => e,
    }
}

/// `dk_project_status` (export of the file): the state bits.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_project_status() -> u32 {
    (*addr_of!(PROJECT)).status()
}
