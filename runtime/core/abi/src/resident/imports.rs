//! `ImportRows`: the module imports the resident core serves (`sdk/sdk.toml` `[[import]]`), as the address of each function. The package's import table (`imports.rs`)
//! lists every name a module may import; for these it holds the field of this struct, and for the others the address of its own function. The resident core checks
//! each address against the function type `sdk/sdk.toml` gives the import (`resident/imports.rs`); a host test, which links no firmware, has a null address for
//! the imports that reach it.

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ImportRows {
    pub memcpy: *const (),
    pub memset: *const (),
    pub memcmp: *const (),
    pub muldi3: *const (),
    pub udivdi3: *const (),
    pub umoddi3: *const (),
    pub dk_alloc: *const (),
    pub dk_free: *const (),
    pub dk_log: *const (),
    pub dk_log_event: *const (),
    pub dk_sink_set: *const (),
    pub dk_span_sink_set: *const (),
    pub dk_time_blocks: *const (),
    pub dk_fw_alloc: *const (),
    pub dk_fw_free: *const (),
    pub dk_fw_block_read: *const (),
    pub dk_io_post: *const (),
    pub dk_io_cancel: *const (),
    pub dk_io_wake: *const (),
    pub dk_helper_run: *const (),
    pub dk_helper_wait: *const (),
    pub dk_job_post: *const (),
    pub dk_job_cancel: *const (),
    pub dk_hash32_init: *const (),
    pub dk_hash32_update: *const (),
    pub dk_hash32_finish: *const (),
    pub dk_intro_take: *const (),
    pub dk_debug_peek: *const (),
    pub dk_debug_span_clock: *const (),
    pub dk_debug_poll: *const (),
    pub dk_debug_track_type: *const (),
    pub dk_debug_selected_params: *const (),
    pub dk_debug_top_view: *const (),
    pub dk_debug_setup: *const (),
    pub dk_device_track_count: *const (),
    pub dk_selected_track: *const (),
    pub dk_debug_log_sink_set: *const (),
    pub dk_debug_speed_flags: *const (),
}

// The addresses name resident functions for the whole run of the system; the rows are built once and only read.
unsafe impl Sync for ImportRows {}
