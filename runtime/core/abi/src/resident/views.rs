//! `Views`: pointers to the resident statics the package reads and writes. The resident core owns the statics (host tools peek them by symbol); the package holds the
//! pointers. The layout of what each pointer reaches is part of the major version of the table (`resident.rs`, "Shared views"): `PackageState` (with `Module` and
//! `LogEntry`), `DkrtBootInfo` (generated from `runtime/core/abi.toml`), the name array and the `MAX_MODULES` arrays.
use crate::runtime_state::PackageState;
use crate::state::NAME_LEN;
use crate::MAX_MODULES;
use dkrt_sdk::{DkrtBootInfo, SEL_MAX};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Views {
    /// `DKRT_STATE`.
    pub state: *mut PackageState,
    /// `DKRT_BOOT`.
    pub boot_info: *mut DkrtBootInfo,
    /// The names of the boot's modules (`LOADS` reports them).
    pub boot_names: *const [[u8; NAME_LEN]; SEL_MAX],
    /// `DKRT_PATHS`: the store path each module record was loaded from.
    pub paths: *mut [[u8; 64]; MAX_MODULES],
    /// `DKRT_MENUS`: the address of each record's `menu`.
    pub menus: *mut [u32; MAX_MODULES],
    /// `DKRT_MENU_OBJS`: whether a menu object was built for each record.
    pub menu_objs: *mut [u8; MAX_MODULES],
    /// `DKRT_MODULE_RUNTIME`: the runtime version each record needs.
    pub module_runtime: *mut [u32; MAX_MODULES],
}

// The pointers name resident statics for the whole run of the system; the views are built once and only read.
unsafe impl Sync for Views {}
