//! The views of the resident statics.
use crate::boot::{BOOT_NAME, DKRT_BOOT};
use crate::state::{DKRT_MENUS, DKRT_MENU_OBJS, DKRT_MODULE_RUNTIME, DKRT_PATHS, DKRT_STATE};
use core::ptr::{addr_of, addr_of_mut};
use dkrt_abi::resident::Views;

pub(super) static VIEWS: Views = Views {
    state: addr_of_mut!(DKRT_STATE),
    boot_info: addr_of_mut!(DKRT_BOOT),
    boot_names: addr_of!(BOOT_NAME),
    paths: addr_of_mut!(DKRT_PATHS),
    menus: addr_of_mut!(DKRT_MENUS),
    menu_objs: addr_of_mut!(DKRT_MENU_OBJS),
    module_runtime: addr_of_mut!(DKRT_MODULE_RUNTIME),
};
