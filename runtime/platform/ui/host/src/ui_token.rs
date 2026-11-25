use crate::*;

/// The handle of `dk_ui_open`: only a token, as in core/ui.
pub(crate) static UI_TOKEN: u8 = 0;

/// # Safety
/// `out` is null or points at a writable pointer.
#[no_mangle]
pub unsafe extern "C" fn dk_ui_open(ctx: *mut c_void, out: *mut *mut c_void) -> i32 {
    if ctx.is_null() || out.is_null() {
        return -48;
    }
    unsafe { *out = &UI_TOKEN as *const u8 as *mut c_void };
    0
}
