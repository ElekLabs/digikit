//! The views of the resident statics.
use crate::boot::{BOOT_NAME, DKRT_BOOT};
use crate::state::{DKRT_MENUS, DKRT_MENU_OBJS, DKRT_MODULE_RUNTIME, DKRT_PATHS, DKRT_STATE};
use core::ptr::{addr_of, addr_of_mut};
use dkrt_abi::resident::{Group, Views};
use dkrt_abi::runtime_state::PackageState;

pub(super) static VIEWS: Group<Views> = Group::new(Views {
    // the package's view of `State` has the arena's words opaque: the same layout (`runtime_state` tests)
    state: addr_of_mut!(DKRT_STATE) as *mut PackageState,
    boot_info: addr_of_mut!(DKRT_BOOT),
    boot_names: addr_of!(BOOT_NAME),
    paths: addr_of_mut!(DKRT_PATHS),
    menus: addr_of_mut!(DKRT_MENUS),
    menu_objs: addr_of_mut!(DKRT_MENU_OBJS),
    module_runtime: addr_of_mut!(DKRT_MODULE_RUNTIME),
});
