//! The key hook of the service as assembler (`hooks.s`) and the one Rust function it calls, `dk_shortcuts_key`. A host build (the assembly test of the runtime runs the declarations of
//! `service!`) has an empty function for the entry, so that its address exists.
#[cfg(target_arch = "m68k")]
use core::arch::global_asm;

#[cfg(target_arch = "m68k")]
global_asm!(include_str!("hooks.s"));

#[cfg(target_arch = "m68k")]
extern "C" {
    pub fn shortcuts_key_hook();
}
#[cfg(not(target_arch = "m68k"))]
pub extern "C" fn shortcuts_key_hook() {}

/// What the hook asks: 1 when a shortcut took the event (the walker returns at once), 0 when the stock walker runs. C calling convention; called by `hooks.s` in the UI task. It runs no
/// module code: the calls of `on` and `off` are queued and the UI pass makes them.
///
/// # Safety
/// Runs in the UI task, inside the key walker.
#[cfg_attr(not(target_arch = "m68k"), allow(dead_code))]
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_shortcuts_key(code: u32, flags: u32) -> u32 {
    use dkrt_svc_shortcuts::shortcuts::Verdict;
    // no owner (before `init` or after `fini`): the stock walker runs
    match crate::DK_SVC_OWNER.with(|s| s.dispatch(&dkrt_svc_rt::ImportCore, code, flags)) {
        Some(Verdict::Swallow) => 1,
        _ => 0,
    }
}
