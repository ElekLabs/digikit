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

/// The firmware's key event flags (`KeyEvent+0x10`) as `key_flags` of the service: bit 0 is set on a press, bit 3 marks a repeat (finding "key event path": values 0x01 press, 0x09 repeat,
/// 0x10 and 0x00 release **[O]** for the repeat value).
pub fn event_flags(fw: u32) -> u32 {
    use dkrt_svc_shortcuts::shortcuts::key_flags;
    // a release first: bit 0 clear is a release whatever the other bits say
    if fw & 1 == 0 {
        key_flags::RELEASE
    } else if fw & 8 != 0 {
        key_flags::REPEAT
    } else {
        0
    }
}

/// What the hook asks: 1 when a shortcut took the event (the walker returns at once), 0 when the stock walker runs. C calling convention; called by `hooks.s` in the UI task. It runs no
/// module code: the calls of `on` and `off` are queued and the UI pass makes them.
///
/// # Safety
/// Runs in the UI task, inside the key walker.
#[cfg_attr(not(target_arch = "m68k"), allow(dead_code))]
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_shortcuts_key(code: u32, flags: u32) -> u32 {
    use dkrt_svc_shortcuts::shortcuts::Verdict;
    match dkrt_svc_shortcuts::records::on_key(&dkrt_svc_rt::ImportCore, code, event_flags(flags)) {
        Verdict::Swallow => 1,
        Verdict::Pass => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::event_flags;
    use dkrt_svc_shortcuts::shortcuts::key_flags;

    /// A release is classified first: bit 0 clear is a release whatever the other bits say; bit 3 on a press is a repeat.
    #[test]
    fn the_firmware_flags_become_release_repeat_or_press() {
        assert_eq!(event_flags(0x01), 0, "press");
        assert_eq!(event_flags(0x03), 0, "press with FUNC");
        assert_eq!(event_flags(0x09), key_flags::REPEAT);
        assert_eq!(event_flags(0x10), key_flags::RELEASE);
        assert_eq!(event_flags(0x00), key_flags::RELEASE);
        assert_eq!(event_flags(0x08), key_flags::RELEASE, "bit 0 clear is a release even with bit 3 set");
    }
}
