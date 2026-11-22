//! The resident table as the package sees it: its header, its views and the shape of a call through it.

use super::TABLE;
use crate::package::resident as facade;
use crate::test_lock::LOCK;
use core::ptr::{addr_of, addr_of_mut};
use dkrt_abi::package_platform::PackagePlatform;
use dkrt_abi::resident::{ResidentTable, RESIDENT_MAGIC, RESIDENT_MAJOR};

#[test]
fn the_header_says_what_this_build_made() {
    assert_eq!(TABLE.header.magic, RESIDENT_MAGIC);
    assert_eq!(TABLE.header.major, RESIDENT_MAJOR);
    assert_eq!(TABLE.header.size as usize, core::mem::size_of::<ResidentTable>());
}

#[test]
fn the_views_name_the_statics_that_host_tools_peek() {
    let v = &TABLE.views;
    assert!(core::ptr::eq(v.state, addr_of_mut!(crate::state::DKRT_STATE)));
    assert!(core::ptr::eq(v.boot_info, addr_of_mut!(crate::boot::DKRT_BOOT)));
    assert!(core::ptr::eq(v.boot_names, addr_of!(crate::boot::BOOT_NAME)));
    assert!(core::ptr::eq(v.paths, addr_of_mut!(crate::state::DKRT_PATHS)));
    assert!(core::ptr::eq(v.menus, addr_of_mut!(crate::state::DKRT_MENUS)));
    assert!(core::ptr::eq(v.menu_objs, addr_of_mut!(crate::state::DKRT_MENU_OBJS)));
    assert!(core::ptr::eq(v.module_runtime, addr_of_mut!(crate::state::DKRT_MODULE_RUNTIME)));
}

/// An entry with an out parameter or a status code gives what the direct call gives: the clock, the arena figures, an unknown file.
#[test]
fn a_call_through_the_table_gives_what_the_direct_call_gives() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    let _arena = crate::fake_platform::test_arena::ScriptedArena::up();
    unsafe {
        let direct = &crate::platform::PLATFORM;
        let table = &facade::platform::PLATFORM;
        assert_eq!(PackagePlatform::blocks(table), PackagePlatform::blocks(direct));
        assert_eq!(PackagePlatform::device_id(table), PackagePlatform::device_id(direct));
        assert_eq!(PackagePlatform::has_dsp(table), PackagePlatform::has_dsp(direct));
        assert_eq!(facade::arena::stats(), crate::arena::stats());
        assert!(core::ptr::eq(facade::state::st(), crate::platform::st()));
        // an error code comes back as the same `Err`
        let path = b"no/such/file";
        assert_eq!(
            PackagePlatform::stat(table, dkrt_abi::storage::ROOT_STORE, path).err(),
            PackagePlatform::stat(direct, dkrt_abi::storage::ROOT_STORE, path).err()
        );
    }
}
