//! dkrt-abi: the interfaces of the digikit runtime (docs/ARCHITECTURE.md, "Three interfaces") and the pure types and constants they share.
//!
//! - `core_api`: interface C, `CoreApi`: what a service calls in the core (parameter rows, the module table, settings, the log, jobs, the DSP link).
//! - `core_imports`: interface C as the `dkc_*` imports of a service file (one macro list; `dkrt-svc-rt` and the runtime expand it).
//! - `service`: interface B, `Service`, `ServiceRow` and `ServiceTable`: what the core calls on a service, with the context types of those calls.
//! - `platform`: `Platform`, the interface to the device and the firmware under the core, with `DeviceProfile` (`profiles`: one generated constant per device file).
//! - `plan`: `Verdict` and `Reason`, the boot planner's answer for a module (R7).
//! - `state`: the state of each module of the boot selection (R11: `BootStates`, `classify`) and the `LOADS` v2 reply bytes.
//! - `fw`: `Fw`, the firmware's parameter page as a service sees it (the runtime implements it; tests fake it).
//! - `keys`: the copy of a module's header table that the commit reads (`KeysCopy`).
//! - `hooks`: `PackageHooks`, what the resident core calls on the system package (one typed entry per crossing, grouped by owner; a missing entry has a default in the core).
//! - `dev_addr`: `DevAddr`, a device address as its own type (the host maps it through an installed `AddressSpace`).
//! - `module`: `Module`, `Ctx` and the names of the log (`LOG_NAME`, `PROVIDER_IDX`); `boot_path`: the selection index a file takes; `fw_id`: the image's firmware identity; `store_path`: string rules of store paths.
//! - `dsp`, `registry`, `mem`: types, constants and helpers with no state. The core owns the objects (`DKRT_DSP`, `DKRT_REG`) and hands them out through `CoreApi`.
//!
//! This crate never depends on the runtime crate: a function that needs the core's state belongs to `CoreApi`, so the dependency arrows between the core and the
//! services point at this crate and nowhere else. `#![no_std]`; no allocation, no vtables.
#![no_std]
#![deny(improper_ctypes, improper_ctypes_definitions)]

pub mod boot_path;
pub mod core_api;
pub mod core_imports;
pub mod dev_addr;
pub mod dsp;
pub mod fw;
pub mod fw_id;
pub mod gen_svc_abi;
pub mod hooks;
pub mod keys;
pub mod link;
pub mod mem;
pub mod module;
pub mod package;
pub mod plan;
pub mod platform;
pub mod profiles;
pub mod provider;
pub mod regcheck;
pub mod registry;
pub mod service;
pub mod state;
pub mod storage;
pub mod store_path;
pub mod version;

/// Module records of the runtime: the selection record's size (`SEL_MAX`, `[limits.max] modules` of sdk/sdk.toml); the device files' `[limits] modules` may not exceed it.
pub const MAX_MODULES: usize = dkrt_sdk::SEL_MAX;
/// Module records and the records of the provider files (`dkrt_sdk::PROVIDER_MAX`): what the loader keeps per loaded file.
pub const REC_MAX: usize = MAX_MODULES + dkrt_sdk::PROVIDER_MAX;
