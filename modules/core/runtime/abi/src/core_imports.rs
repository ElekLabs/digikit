//! Interface C as the imports of a service file (docs/plans/runtime-and-services.md section 13.2): one `extern "C" fn dkc_*` per `CoreApi` method, in C shape, and
//! the two imports only a service file makes (`dk_service_declare_row`, `dkc_panic`). The list is one macro; two crates expand it:
//!
//! - `dkrt-svc-rt` declares the names in an `extern "C"` block and implements `CoreApi` over them (`ImportCore`);
//! - the runtime checks one function of its own for each entry against the declared type and builds its second import table `SVC_IMPORTS` from the names.
//!
//! The shape rules: a `&'static mut T` is a `*mut T`, a slice is a pointer and a length, an `Option` result is a `bool` and an out pointer, a struct that is
//! passed or returned by reference is a `*const` or `*mut` of its `repr(C)` type, an array of words is a pointer. A result that is a `bool` stays a `bool`
//! (`extern "C"` and Rust on both sides).
//!
//! This file is one of the boundary files `patchkit gen` hashes into `SVC_ABI`: any change here changes the fingerprint.
pub use dkrt_sdk::api::{JobDoneFn, JobStepFn};

/// Invoke `$m! { fn name(arg: Ty, ...) -> Ret; ... }` with the imports of a service file: the `dkc_*` of `CoreApi`, then `dk_service_declare_row` and `dkc_panic`.
#[macro_export]
macro_rules! core_imports {
    ($m:ident) => {
        $m! {
            // ---- the registry's rows
            fn dkc_boot_index() -> u32;
            // `out` points at 8 words; the count of keys written
            fn dkc_keys_of(at: u32, kind: u32, out: *mut u32) -> u32;
            // `out` points at 32 bytes
            fn dkc_legacy_state(out: *mut u8);
            fn dkc_legacy_state_clear() -> i32;
            fn dkc_live_op() -> u32;
            // ---- the device
            fn dkc_device_id() -> u32;
            // ---- the module table
            fn dkc_module_name(owner: u8) -> *const u8;
            // The module whose context `ctx` is, when it is the code running now: record index + 1, else 0.
            fn dkc_module_of(ctx: *const u8) -> u8;
            // ---- the files: `out` is a StatWire; stat_head gives the head byte count (0 or 64) or a negative code, `head` is 64 bytes
            fn dkc_storage_stat(root: u32, path: *const u8, len: u32, out: *mut $crate::storage::StatWire) -> i32;
            fn dkc_storage_stat_head(root: u32, path: *const u8, len: u32, out: *mut $crate::storage::StatWire, head: *mut u8) -> i32;
            // ---- the interrupt mask
            fn dkc_irq_mask() -> u32;
            fn dkc_irq_restore(sr: u32);
            // ---- settings
            fn dkc_settings_get(key: *const u8, key_len: u32, out: *mut u8, out_len: u32) -> i32;
            fn dkc_settings_put(key: *const u8, key_len: u32, value: *const u8, value_len: u32) -> i32;
            // ---- log and source: `args` points at five words
            fn dkc_log(kind: u16, flags: u16, args: *const u32);
            fn dkc_current_source() -> u8;
            // ---- jobs
            fn dkc_job_post(owner: u8, step: Option<$crate::core_imports::JobStepFn>, done: Option<$crate::core_imports::JobDoneFn>, ctx: *mut u8) -> i32;
            fn dkc_job_cancel(owner: u8, id: u32) -> i32;
            // ---- the DSP loader's state and tables
            fn dkc_dsp() -> *mut $crate::dsp::DspState;
            // An entry of the image's info table by the `name_hash` of its name (`CoreApi::info`): null when the image has none.
            fn dkc_info(name_hash: u32) -> *const u32;
            // ---- the DSP link
            fn dkc_link_set(slot: *const $crate::link::Slot, value: u32);
            // `true`: `*out` is the confirmed value.
            fn dkc_link_confirmed(slot: *const $crate::link::Slot, out: *mut u32) -> bool;
            fn dkc_link_errors() -> u32;
            fn dkc_link_forget(win: u32, first: u32, count: u32);
            // `true`: `*out` is the ticket.
            fn dkc_link_submit(owner: $crate::link::Owner, stream: *const $crate::link::Stream, out: *mut $crate::link::Ticket) -> bool;
            fn dkc_link_result(ticket: *const $crate::link::Ticket) -> $crate::link::LinkResult;
            fn dkc_link_drain(owner: $crate::link::Owner) -> bool;
            fn dkc_link_run(owner: $crate::link::Owner, seq: *const $crate::link::Sequence) -> bool;
            // ---- only a service file: its row, and a panic
            // Called in `init`: the `ServiceRow` this file provides (`ctx` is the module context). A status.
            fn dk_service_declare_row(ctx: *mut u8, row: *const $crate::service::ServiceRow) -> i32;
            // A service panicked: the runtime's panic (crash capture, safe mode on the next boot). Never returns.
            fn dkc_panic() -> !;
        }
    };
}
