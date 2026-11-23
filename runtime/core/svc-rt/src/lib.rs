//! dkrt-svc-rt: what a service file (`.dks`) links to talk to the runtime (docs/plans/runtime-and-services.md section 13.2).
//!
//! - `import_core`: `ImportCore`, a zero-size `CoreApi` over the `dkc_*` imports of `dkrt_abi::core_imports`. A `Service<C: CoreApi>` runs over it unchanged; the static rows
//!   of the runtime run the same code over `RealCore`, the host tests over `FakeCore`.
//! - `service`: the `service!` macro: the module record `dk_module` (its `init` declares the service's record kinds, its `ServiceRow` and, with `setup =` and `hooks =`, its hook sites:
//!   `("cf.page.hook.page_desc", hooks::fxp_acc)` for every device, `(name, entry, dt2)` or `(name, entry, dn2)` for one; see `modules/README.md` "Services"), the data word `dk_svc_abi` the packer
//!   copies to header word 0x3c, and the panic handler (the runtime's panic: crash capture, safe mode on the next boot).
//! - `declare`: the `dk_service_declare_*` calls of `init`, the hook device filter and the module context of the file.
//! - `module_def`: the record `dk_module` (`ModuleDef`).
//!
//! `#![no_std]`. The boundary is `extern "C"` with `repr(C)` types only (`improper_ctypes` is denied).
#![no_std]
#![deny(improper_ctypes, improper_ctypes_definitions)]

mod declare;
mod import_core;
mod module_def;
mod service;
#[cfg(test)]
mod tests;

pub use declare::{
    declare, declare_resident_query, init, init_ctx, name_hash, running_device, set_init_ctx,
    DEVICE_DN2, DEVICE_DT2,
};
pub use dkrt_abi;
pub use import_core::{sys, ImportCore};
pub use module_def::{ModuleDef, TickFn};
