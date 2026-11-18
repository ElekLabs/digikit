//! The exports that only the runtime calls (`internal` imports of the `machines` service in sdk/sdk.toml, `wrapper = "rt"`): the runtime finds them by name when the
//! MODULES reply needs the machine types of a module and their guard counts (`handlers::exports`). Plain `extern "C"` functions with no core argument: the file reaches
//! the core through `ImportCore`, as the builder exports do.
use dkrt_abi::service::GuardState;
use dkrt_svc_machines::records;
use dkrt_svc_rt::ImportCore;

/// The guard's counts of machine type `ty` into the `GuardState` at `out`; 1 (the machines service owns every type it is asked about).
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_machines_guard_state(ty: u32, out: *mut u8) -> u32 {
    *(out as *mut GuardState) = records::guard_state(&ImportCore, ty);
    1
}

/// The machine type numbers of module record `owner` into the 8 bytes at `out`; how many.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_machines_owned_ids(owner: u32, out: *mut u8) -> u32 {
    records::owned_ids(&ImportCore, owner as u8, &mut *(out as *mut [u8; 8]))
}
