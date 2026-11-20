//! The export that only the runtime calls (`internal` import of the `machines` service in sdk/sdk.toml, `wrapper = "rt"`): the runtime finds it by name when the
//! MODULES reply needs the machines of a module (`handlers::exports`). A plain `extern "C"` function with no core argument: the file reaches the core through
//! `ImportCore`, as the builder exports do.
use dkrt_abi::service::ModuleMachines;
use dkrt_svc_machines::records;
use dkrt_svc_rt::ImportCore;

/// The machines of module record `owner` into the `ModuleMachines` at `out` (the voice rows it owns, its machine types, the guard's counts for them); 1 (the machines
/// service answers for every owner it is asked about).
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_machines_module_info(owner: u32, out: *mut u8) -> u32 {
    *(out as *mut ModuleMachines) = records::module_info(&ImportCore, owner as u8);
    1
}
