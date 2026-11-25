//! The key hook of the fixture as assembler (`hooks.s`). A host build has an empty function for the entry, so that its address exists.
#[cfg(target_arch = "m68k")]
use core::arch::global_asm;

#[cfg(target_arch = "m68k")]
global_asm!(include_str!("hooks.s"));

#[cfg(target_arch = "m68k")]
extern "C" {
    pub fn local_step_key_hook();
}
#[cfg(not(target_arch = "m68k"))]
pub extern "C" fn local_step_key_hook() {}
