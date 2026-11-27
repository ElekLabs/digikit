//! The routines of the five hooks of the stock project paths as assembler (`hooks.s`, the same code for both devices) and the record they write: `DKRT_PROJECT_PENDING`
//! (`dkrt_svc_project::engine::Pending`). A hook only records; the tick of the file (`glue::tick`) takes the record. A host build (the assembly test of the runtime runs the
//! declarations of `service!`) has an empty function for each entry, so that the entry addresses exist.
use dkrt_svc_project::engine::Pending;

#[cfg(target_arch = "m68k")]
use core::arch::global_asm;

#[cfg(target_arch = "m68k")]
global_asm!(include_str!("hooks.s"));

/// What the hooks wrote since the last tick.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub static mut DKRT_PROJECT_PENDING: Pending = Pending::new();

macro_rules! entries {
    ($($name:ident,)*) => {
        $(
            #[cfg(target_arch = "m68k")]
            extern "C" {
                pub fn $name();
            }
            #[cfg(not(target_arch = "m68k"))]
            pub extern "C" fn $name() {}
        )*
    };
}

entries! {
    project_hook_slot,
    project_hook_save,
    project_hook_clear_one,
    project_hook_clear_many,
    project_hook_paste,
}
