//! The import table of the modules (`IMPORTS`, one list of names and addresses checked against `sdk/sdk.toml`), the imports of other
//! modules (`optional`), the lookups `import_addr` and `import_needs`, and the imports that are plain functions.
//!
//! A name is either the package's own function (`own`) or a row of the resident table (`row`: the resident core serves it, `dkrt_abi::resident::ImportRows`).

use crate::package::resident::table;
use crate::package::{collect, entry};
use core::mem::offset_of;
use dkrt_abi::dev_addr::DevAddr;
use dkrt_abi::mem::cstr_eq;
use dkrt_abi::resident::ImportRows;

// ---- imports -------------------------------------------------------------------------------------------------------

/// A name a module may import. The address is the package's own function, or, for a row of the resident table (`row` not 0), the field of `ImportRows` at byte
/// offset `row - 1`.
#[repr(C)]
pub struct Import {
    pub name: [u8; 32],
    pub addr: *const (),
    row: u16,
}

impl Import {
    const fn own(name: &[u8], addr: *const ()) -> Import {
        Import { name: nm(name), addr, row: 0 }
    }

    const fn row(name: &[u8], offset: usize) -> Import {
        Import { name: nm(name), addr: core::ptr::null(), row: offset as u16 + 1 }
    }

    /// The address a module gets: the function of the package or of the resident core.
    pub fn address(&self) -> *const () {
        if self.row == 0 {
            self.addr
        } else {
            unsafe { *((&table().imports as *const ImportRows as *const u8).add(self.row as usize - 1) as *const *const ()) }
        }
    }
}
pub struct Imports(pub [Import; dkrt_sdk::api::BUILTIN_NAMES.len()]);
unsafe impl Sync for Imports {}

const fn nm(s: &[u8]) -> [u8; 32] {
    let mut o = [0u8; 32];
    let mut i = 0;
    while i < s.len() {
        o[i] = s[i];
        i += 1;
    }
    o
}

const fn names_match(ours: &[&[u8]], sdk: &[&str]) -> bool {
    if ours.len() != sdk.len() {
        return false;
    }
    let mut i = 0;
    while i < ours.len() {
        let (a, b) = (ours[i], sdk[i].as_bytes());
        if a.len() != b.len() {
            return false;
        }
        let mut j = 0;
        while j < a.len() {
            if a[j] != b[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// One entry of the table: `own` with the package's function, `row` with the field of `ImportRows` that holds the resident function.
macro_rules! import_entry {
    ($name:literal, own $f:expr) => {
        Import::own($name.as_bytes(), $f as *const ())
    };
    ($name:literal, row $field:ident) => {
        Import::row($name.as_bytes(), offset_of!(ImportRows, $field))
    };
}

/// `NAMES` (the names of `IMPORTS`, in its order) and `IMPORTS` from one list, so a name and its address cannot drift apart; the names are checked against
/// the `[[import]]` entries of `sdk/sdk.toml` without `requires`.
macro_rules! import_table {
    ($(($name:literal, $kind:ident $($what:tt)*)),* $(,)?) => {
        const NAMES: [&[u8]; dkrt_sdk::api::BUILTIN_NAMES.len()] = [$($name.as_bytes()),*];
        /// The names a module may import. Firmware functions come from patch-time anchors (`dkfw`) through the resident table; the helpers are the resident
        /// core's own copies, so a module need not carry them. The `dk_*` services are the runtime's own functions (`entry.rs` for the ones that take a handle).
        pub static IMPORTS: Imports = Imports([$(import_entry!($name, $kind $($what)*)),*]);
    };
}

import_table! {
    ("memcpy", row memcpy),
    ("memset", row memset),
    ("memcmp", row memcmp),
    ("__muldi3", row muldi3),
    ("__udivdi3", row udivdi3),
    ("__umoddi3", row umoddi3),
    ("dk_alloc", row dk_alloc),
    ("dk_free", row dk_free),
    ("dk_log", row dk_log),
    ("dk_log_event", row dk_log_event),
    ("dk_sink_set", row dk_sink_set),
    ("dk_span_sink_set", row dk_span_sink_set),
    ("dk_time_blocks", row dk_time_blocks),
    ("dk_fw_alloc", row dk_fw_alloc),
    ("dk_fw_free", row dk_fw_free),
    ("dk_fw_block_read", row dk_fw_block_read),
    ("dk_files_read", own entry::dk_files_read),
    ("dk_host_register", own imp_host_register),
    ("dk_module_ctx_device", own entry::dk_module_ctx_device),
    ("dk_module_ctx_runtime_version", own entry::dk_module_ctx_runtime_version),
    ("dk_workbench_register", own super::module_ui::dk_workbench_register),
    ("dk_settings_open", own entry::dk_settings_open),
    ("dk_settings_get", own entry::dk_settings_get),
    ("dk_settings_put", own entry::dk_settings_put),
    ("dk_settings_remove", own entry::dk_settings_remove),
    ("dk_files_open", own entry::dk_files_open),
    ("dk_files_list", own entry::dk_files_list),
    ("dk_files_poll", own entry::dk_files_poll),
    ("dk_files_cancel", own entry::dk_files_cancel),
    ("dk_files_create", own entry::dk_files_create),
    ("dk_files_write", own entry::dk_files_write),
    ("dk_files_close", own entry::dk_files_close),
    ("dk_files_abort", own entry::dk_files_abort),
    ("dk_files_delete", own entry::dk_files_delete),
    ("dk_files_commit", own entry::dk_files_commit),
    ("dk_files_pending", own entry::dk_files_pending),
    ("dk_io_post", row dk_io_post),
    ("dk_io_cancel", row dk_io_cancel),
    ("dk_io_wake", row dk_io_wake),
    ("dk_helper_run", row dk_helper_run),
    ("dk_helper_wait", row dk_helper_wait),
    ("dk_job_post", row dk_job_post),
    ("dk_job_cancel", row dk_job_cancel),
    ("dk_hash32_init", row dk_hash32_init),
    ("dk_hash32_update", row dk_hash32_update),
    ("dk_hash32_finish", row dk_hash32_finish),
    ("dk_intro_take", row dk_intro_take),
    ("dk_record_add", own collect::dk_record_add),
    ("dk_service_declare_kind", own collect::dk_service_declare_kind),
    ("dk_service_declare_slot", own collect::dk_service_declare_slot),
    ("dk_service_declare_hook", own collect::dk_service_declare_hook),
    ("dk_service_declare_export", own collect::dk_service_declare_export),
    ("dk_service_declare_resource", own collect::dk_service_declare_resource),
    ("dk_debug_peek", row dk_debug_peek),
    ("dk_debug_span_clock", row dk_debug_span_clock),
    ("dk_debug_poll", row dk_debug_poll),
    ("dk_dsp_stats_ctl", own crate::package::dsp_stats::dk_dsp_stats_ctl),
    ("dk_dsp_stats_read", own crate::package::dsp_stats::dk_dsp_stats_read),
    ("dk_debug_track_type", row dk_debug_track_type),
    ("dk_debug_selected_params", row dk_debug_selected_params),
    ("dk_debug_top_view", row dk_debug_top_view),
    ("dk_debug_setup", row dk_debug_setup),
    ("dk_device_track_count", row dk_device_track_count),
    ("dk_selected_track", row dk_selected_track),
    ("dk_debug_log_sink_set", row dk_debug_log_sink_set),
    ("dk_debug_speed_flags", row dk_debug_speed_flags),
}

const _: () = assert!(names_match(&NAMES, &dkrt_sdk::api::BUILTIN_NAMES), "IMPORTS names differ from [[import]] of sdk/sdk.toml");
// the imports of this file have the types sdk/sdk.toml gives them (those of the resident core are checked where its rows are built)
const _: dkrt_sdk::api::DkDspStatsCtlFn = crate::package::dsp_stats::dk_dsp_stats_ctl;
const _: dkrt_sdk::api::DkDspStatsReadFn = crate::package::dsp_stats::dk_dsp_stats_read;
const _: dkrt_sdk::api::DkFilesReadFn = entry::dk_files_read;
const _: dkrt_sdk::api::DkHostRegisterFn = imp_host_register;
const _: dkrt_sdk::api::DkModuleCtxDeviceFn = entry::dk_module_ctx_device;
const _: dkrt_sdk::api::DkModuleCtxRuntimeVersionFn = entry::dk_module_ctx_runtime_version;
const _: dkrt_sdk::api::DkSettingsOpenFn = entry::dk_settings_open;
const _: dkrt_sdk::api::DkWorkbenchRegisterFn = super::module_ui::dk_workbench_register;
const _: dkrt_sdk::api::DkSettingsGetFn = entry::dk_settings_get;
const _: dkrt_sdk::api::DkSettingsPutFn = entry::dk_settings_put;
const _: dkrt_sdk::api::DkSettingsRemoveFn = entry::dk_settings_remove;
const _: dkrt_sdk::api::DkFilesOpenFn = entry::dk_files_open;
const _: dkrt_sdk::api::DkFilesListFn = entry::dk_files_list;
const _: dkrt_sdk::api::DkFilesPollFn = entry::dk_files_poll;
const _: dkrt_sdk::api::DkFilesCancelFn = entry::dk_files_cancel;
const _: dkrt_sdk::api::DkFilesCreateFn = entry::dk_files_create;
const _: dkrt_sdk::api::DkFilesWriteFn = entry::dk_files_write;
const _: dkrt_sdk::api::DkFilesCloseFn = entry::dk_files_close;
const _: dkrt_sdk::api::DkFilesAbortFn = entry::dk_files_abort;
const _: dkrt_sdk::api::DkFilesDeleteFn = entry::dk_files_delete;
const _: dkrt_sdk::api::DkFilesCommitFn = entry::dk_files_commit;
const _: dkrt_sdk::api::DkFilesPendingFn = entry::dk_files_pending;
const _: dkrt_sdk::api::DkRecordAddFn = collect::dk_record_add;
const _: dkrt_sdk::api::DkServiceDeclareKindFn = collect::dk_service_declare_kind;
const _: dkrt_sdk::api::DkServiceDeclareSlotFn = collect::dk_service_declare_slot;
const _: dkrt_sdk::api::DkServiceDeclareHookFn = collect::dk_service_declare_hook;
const _: dkrt_sdk::api::DkServiceDeclareExportFn = collect::dk_service_declare_export;
const _: dkrt_sdk::api::DkServiceDeclareResourceFn = collect::dk_service_declare_resource;

// ---- imports of other modules --------------------------------------------------------------------------------------------

// The imports another module provides (`[[import]] requires` of sdk/sdk.toml: core/input, core/ui). The symbols are in this object's `optional`
// list (module.toml): patchkit links one to 0 when its module is not in the image.
dkrt_sdk::optional_imports!();

/// Address of the import `name` (NUL-terminated), 0 when the table has none. An import of another module (`optional`) gives that module's
/// function; when the module is not in the image it gives the stub of the import if sdk/sdk.toml declares `absent`, else 0.
pub unsafe fn import_addr(name: *const u8) -> u32 {
    let mut i = 0;
    while i < IMPORTS.0.len() {
        if cstr_eq(IMPORTS.0[i].name.as_ptr(), name) {
            return DevAddr::of_fn(IMPORTS.0[i].address() as usize).get();
        }
        i += 1;
    }
    let t = optional::table(crate::package::resident::links::optional());
    i = 0;
    while i < t.len() {
        if cstr_eq_str(name, t[i].0) {
            return DevAddr::of_fn(if t[i].1 != 0 { t[i].1 } else { t[i].2 }).get();
        }
        i += 1;
    }
    0
}

/// The module whose absence leaves the import `name` (NUL-terminated) without an address, without the `core/` prefix ("usb-bulk" for an import of core/usb-bulk
/// that has no stub); None when the table has no such import, or it has an address or a stub. Every import of another module has a stub today.
pub unsafe fn import_needs(name: *const u8) -> Option<&'static str> {
    let t = optional::table(crate::package::resident::links::optional());
    let mut i = 0;
    while i < t.len() {
        if cstr_eq_str(name, t[i].0) {
            if t[i].1 != 0 || t[i].2 != 0 {
                return None;
            }
            let r = t[i].3;
            return Some(match r.rfind('/') {
                Some(at) => &r[at + 1..],
                None => r,
            });
        }
        i += 1;
    }
    None
}

/// Whether the NUL-terminated `a` equals `b`.
unsafe fn cstr_eq_str(a: *const u8, b: &str) -> bool {
    let b = b.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if *a.add(i) != b[i] {
            return false;
        }
        i += 1;
    }
    *a.add(b.len()) == 0
}



// ---- the imports that are plain functions ----------------------------------------------------------------------------------

unsafe extern "C" fn imp_host_register(first: u32, count: u32, handler: Option<dkrt_sdk::HostCmdFn>) -> i32 {
    crate::package::module_cmds::host_register(first, count, handler)
}
