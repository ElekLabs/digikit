//! The key routine of the service: the one function it subscribes to the platform's key walk (`core/input`, `input_keys_info`). The walker calls it as `uint32_t f(void *vc, const void *ev)` in
//! the UI task, for every key event of the panel, before any view.
use core::ffi::c_void;

/// The words of the firmware's `KeyEvent` that the routine reads: the key code at +0xc and the flags at +0x10 (ports/fw/input.toml, `cf.input.site.key_event_walk`).
const KEY_CODE_WORD: usize = 3;
const KEY_FLAGS_WORD: usize = 4;

/// What the routine answers: 1 when a shortcut took the event (the walker stops there and returns 1 to its caller, so no later subscriber and no view sees it), 0 when the event goes on. It runs
/// no module code: the calls of `on` and `off` are queued and the UI pass makes them.
///
/// # Safety
/// Runs in the UI task, inside the key walker: `ev` points at the firmware's `KeyEvent`.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_shortcuts_key(_vc: *mut c_void, ev: *const c_void) -> u32 {
    use dkrt_svc_shortcuts::shortcuts::Verdict;
    let words = ev as *const u32;
    let (code, flags) = (
        words.add(KEY_CODE_WORD).read(),
        words.add(KEY_FLAGS_WORD).read(),
    );
    // no owner (before `init` finished or after `fini`), or a call of the service is running already (a module's callback that reaches the key walker): the event goes on and the service
    // never sees the key; the second case is counted in `DK_SVC_OWNER.refused()`.
    match crate::DK_SVC_OWNER.with(|s| s.dispatch(&dkrt_svc_rt::ImportCore, code, flags)) {
        Some(Verdict::Swallow) => 1,
        _ => 0,
    }
}
