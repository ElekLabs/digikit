//! Interface C as the imports of a service file (docs/plans/runtime-and-services.md section 13.2): one `extern "C" fn dkc_*` per `CoreApi` method, in C shape, and
//! the imports only a service file makes (`dk_provider_declare_row`, `dkc_panic`, `dk_service_declare_patch`). The list is one macro; two crates expand it:
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
pub use dkrt_sdk::DkrtLive;

/// Invoke `$m! { fn name(arg: Ty, ...) -> Ret; ... }` with the imports of a service file: the `dkc_*` of `CoreApi`, then `dk_provider_declare_row` and `dkc_panic`.
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
            // ---- the settings overlay: the table the runtime's settings calls forward to (`dkrt_abi::service::SettingsOverlay`; the runtime drops it with the file)
            fn dkc_settings_overlay_set(overlay: *const $crate::service::SettingsOverlay);
            // The settings of module `id` (`id_len` bytes) changed behind the module's back: the runtime reloads the module if it opened its settings.
            fn dkc_settings_changed(id: *const u8, id_len: u32);
            // ---- the live job (`dkrt_live`, `dkrt_live_state`; UI task only): queue a load, unload or reload of the file at `path` (NUL terminated) and read the job's state
            fn dkc_live(op: u32, path: *const u8) -> i32;
            fn dkc_live_state(out: *mut $crate::core_imports::DkrtLive);
            // Add the store file at `path` (`len` bytes) to the saved selection, as the menu does for a module it loads live.
            fn dkc_selection_add(path: *const u8, len: u32) -> i32;
            // ---- the loaded modules and the store's module files
            // The NUL-terminated store path of the content module in record `owner - 1`; null when the record holds none.
            fn dkc_module_path(owner: u8) -> *const u8;
            // The store path of the file `name` under `dir`: its length (the path NUL padded in the 64 bytes at `out`) or a negative code.
            fn dkc_store_find(dir: *const u8, dir_len: u32, name: *const u8, name_len: u32, out: *mut u8) -> i32;
            // ---- only a service file: its row, and a panic
            // Called in `init` after `Provider::start`: the `ProviderRow` this file provides and the instance (the owner object) its part functions work on. A status.
            fn dk_provider_declare_row(ctx: *mut u8, row: *const $crate::provider::ProviderRow, instance: *mut u8) -> i32;
            // A service panicked: the runtime's panic (crash capture, safe mode on the next boot). Never returns.
            fn dkc_panic() -> !;
            // ---- appended: reload the loaded module `id` (`id_len` bytes) when possible, whether it opened its settings or not (`CoreApi::reload_when_possible`)
            fn dkc_reload_when_possible(id: *const u8, id_len: u32);
            // Called in `init` after `Provider::start`, beside `dk_service_declare_hook`: a patch the service takes, `len` (1, 2 or 4) bytes of `value` (big-endian, the low bytes) at the
            // patch site `site` (the `name_hash` of the `[[patch]]` name). The values are run-time facts (table addresses, counts), so they come from `start`, not from the file. A status.
            fn dk_service_declare_patch(ctx: *mut u8, site: u32, value: u32, len: u32) -> i32;
        }
    };
}
