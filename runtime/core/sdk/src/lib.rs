//! dkrt-sdk: the C ABI between the digikit runtime (`core/runtime`) and the modules it loads at run time.
//!
//! Shared by the runtime crate and by module crates (path dependency). `#![no_std]`; every function pointer is a
//! gcc m68k cdecl function (arguments on the stack, result in d0); there are no small-struct results.
//! Format and rules: docs/design/digikit-runtime.md and docs/design/digikit-dkm.md.
#![no_std]

/// The one version of the SDK: the runtime release. A module records the version it needs in the `.dkm` header (the newest `since` of its imports). 2: DSP sections and asynchronous loads
/// (finding 32), `register` PAGE records and `own_dsp` (finding 33), codes -25/-26 and -30 to -37. 3: card safety (codes -46/-47, -62), the
/// debug log and its SysEx commands LOG and PEEK, `dkrt_diag`, `dkrt_debug_set`, the crash record (finding 46). 4: SELECT and RESTART.
/// 5: the DSP guard (finding 51): voice rows of 16 words with the knob ranges, DSP sections of version 2 (voices of 48 bytes), the 24-word
/// DSP diagnostics with the guard's fault words, the log events `MACHINE_FAULT` and `MACHINE_DISABLED`.
/// Versions 1 to 5 were plain integers (they read as 0.0.1 to 0.0.5 packed).
/// 0.1.0 = semantic version (a module needs the same major and minor and at most this patch); LOGCFG, log levels and sources, `dk_log` with a level.
pub use gen_sdk::RUNTIME_VERSION;

/// The structs, constants and function types of the runtime's C ABI, written by `patchkit gen` from `core/runtime/abi.toml` (also the C header
/// `core/runtime/include/dkrt.h`): `OK`, `LOADING`, `Dkrt*`, `HEALTH_*`, `SAFE_*`, `CARD_*`, `SCAN_*`, `sig`.
#[rustfmt::skip]
pub mod gen_abi;
pub use gen_abi::*;

/// Reserved name of a resident query v1 entry in a service file.
pub const RESIDENT_QUERY_ENTRY: &str = "dk_service_query_v1";

/// The fixed v1 entry of a provider's resident query facet. Every operation has its own typed input and output.
pub type ResidentQueryV1 = unsafe extern "C" fn(u32, *const u8, u32, *mut u8, u32) -> i32;

// Result codes of `dkrt_load` and `dk_files_read` (negative = error; `OK` is in `gen_abi`). The codes themselves are written by `patchkit gen` from `[load.status]` of `sdk/sdk.toml` (`gen_sdk.rs`, module `load`, with their
// texts): -1 to -14 `dkrt_load`; -20 to -29 the run-time registry (`register` records, docs/design/digikit-registry.md); -30 to -39 the
// DSP side's (docs/design/digikit-dkm.md, "DSP section"); -40 to -49 boot-time loading (finding 37). Additive: a code is never renumbered.
pub use gen_sdk::load::*;

// The import names, their function types and the callback types are written by `patchkit gen` from `[[import]]` and `[handle.callbacks]` of `sdk/sdk.toml` (`gen_sdk.rs`, module `api`).
pub use gen_sdk::api;
pub use gen_sdk::api::HostCmdFn;
pub use gen_sdk::keys;
pub use gen_sdk::records;
pub use gen_sdk::sample_pool;

// `DkrtStatus`, `DkrtCap`, `DkrtHealth`, `DkrtStoreEntry`, `DkrtModule`, `DkrtFx`, `DkrtSelection`, `DkrtBootInfo`, `DkrtDiag` and the scan and fit
// structs, with `STATE_*`, `MOD_*`, `CAP_MAX`, `HEALTH_*`, `SEL_PATH`, `SAFE_*`, `CARD_*` and `DIAG_MAGIC`, are in `gen_abi`.

/// The module and sample store, version 3 (docs/design/digikit-store.md).
pub mod store;

/// Bounded native sample validation and integer-only PCM decoding.
pub mod sample;
pub mod sample_job;
pub mod sample_tracks;

pub mod files;
/// Internal fixed-memory listing work/notification lifecycle model.
pub mod files_job;

/// The sticky machine type assignment (docs/design/module-sdk.md 11.5 D7) as a pure function, tested on the host; the runtime's `assign` calls it.
pub mod sticky;

/// The loader's limit decisions (DSP placement, row counts, refusal codes) as pure functions, tested on the host; the runtime calls them.
pub mod capacity;

/// The settings record, two copies before the selection record (docs/design/digikit-store.md, "The settings record").
pub mod settings;

/// The one rule of resident table versions, shared by the package header and the table check.
pub mod table_version;

/// The header of the system package, shared by the resident stage 1 and patchkit's packer.
pub mod syspkg;

/// The last fault (a CPU crash or a DSP stop) as the settings key `dk/fault`, and the ABOUT rows made from it.
pub mod fault;

// `CARD_*` (the state of the card's store area as the runtime found it at boot), `DkrtDiag` and `DIAG_MAGIC` are in `gen_abi`
// (docs/findings/46-digikit-debug.md).

/// Event kinds of the debug log (docs/findings/46-digikit-debug.md has the argument meanings; `tools/device/dklog.py` the readable names).
/// An event is 32 bytes: `seq`, `time` (the firmware's block counter, 1500 per second), `kind`, `flags` (bit 0: an error, kept when debug is
/// off), five argument words.
/// Written by `patchkit gen` from `sdk/sdk.toml` (`gen_sdk.rs`), with the runtime version.
#[rustfmt::skip]
mod gen_sdk;
pub use gen_sdk::{limits, logk, proto, OPTIONAL_COUNT};

mod command;
mod mailbox;
mod result_text;
/// The selection record.
pub mod sel;
#[cfg(test)]
mod tests;

pub use command::*;
pub use mailbox::Mailbox;
pub use result_text::result_text;
