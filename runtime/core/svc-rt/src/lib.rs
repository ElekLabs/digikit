//! dkrt-svc-rt: what a service file (`.dks`) links to talk to the runtime (docs/plans/runtime-and-services.md section 13.2).
//!
//! - `import_core`: `ImportCore`, a zero-size `CoreApi` over the `dkc_*` imports of `dkrt_abi::core_imports`. A part trait over `C: CoreApi` runs over it unchanged; the host tests
//!   run the same code over `FakeCore`.
//! - `service`: the `service!` macro: the module record `dk_module` (its `init` starts the owner object, declares the service's record kinds, its `ProviderRow` and, with `hooks =`, its hook sites:
//!   `("cf.page.hook.page_desc", hooks::fxp_acc)` for every device, `(name, entry, dt2)` or `(name, entry, dn2)` for one; see `modules/README.md` "Services"), the data word `dk_svc_abi` the packer
//!   copies to header word 0x3c, and the panic handler (the runtime's panic: crash capture, safe mode on the next boot).
//! - `owner`: `Owner<T>`, the cell that holds the owner object of a provider file.
//! - `declare`: the `dk_service_declare_*` and `dk_provider_declare_row` calls of `init`, the hook device filter and the module context of the file.
//! - `module_def`: the record `dk_module` (`ModuleDef`).
//!
//! `#![no_std]`. The boundary is `extern "C"` with `repr(C)` types only (`improper_ctypes` is denied).
#![no_std]
#![deny(improper_ctypes, improper_ctypes_definitions)]

#[cfg(test)]
extern crate std;

mod declare;
mod import_core;
mod module_def;
mod owner;
mod service;
mod shared;
#[cfg(test)]
mod tests;

pub use declare::*;
pub use dkrt_abi;
pub use dkrt_abi::provider::Reentry;
pub use dkrt_sdk;
pub use import_core::{sys, ImportCore};
pub use module_def::{ModuleDef, TickFn};
pub use owner::{Events, NoEvent, NoEvents, Owner, Post};
pub use shared::Shared;
