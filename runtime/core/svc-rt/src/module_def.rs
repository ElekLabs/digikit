//! The record `dk_module` of a service file: the four entry points of `dk_module_def` (<dk/core/module.h>).

/// The record `dk_module` of a service file: the four entry points of `dk_module_def` (<dk/core/module.h>); a missing one is a zero word.
#[repr(C)]
pub struct ModuleDef {
    pub init: Option<unsafe extern "C" fn(*mut u8) -> i32>,
    pub fini: Option<unsafe extern "C" fn(*mut u8)>,
    pub tick: Option<unsafe extern "C" fn(*mut u8)>,
    pub menu: Option<unsafe extern "C" fn(*mut u8, *mut u8)>,
}

// Read-only data.
unsafe impl Sync for ModuleDef {}

/// The `tick` of a service file: called once per UI pass with the module context, as the `tick` of a module (`ModuleDef::tick`).
pub type TickFn = unsafe extern "C" fn(*mut u8);
