//! The resident table as the package sees it: its header, its views, the compatibility check, a table of fake functions and the parity of the thunks with the
//! owners.

mod append;
mod check;
mod fake;
mod groups;
mod parity;

use super::TABLE;
use core::ptr::{addr_of, addr_of_mut};
use dkrt_abi::resident::{check as check_table, Resident, ResidentTable, RESIDENT_MAGIC, RESIDENT_MAJOR, RESIDENT_MINOR};

/// The groups of the real table, as the package gets them.
fn real() -> Resident {
    open(&TABLE, RESIDENT_MINOR).expect("the table of this build fits it")
}

/// The groups of `table` for a package built against `built_minor`, or the refusal.
fn open(table: &ResidentTable, built_minor: u16) -> Result<Resident, dkrt_abi::resident::Refusal> {
    let mut into = core::mem::MaybeUninit::<Resident>::uninit();
    unsafe { check_table(table, built_minor, into.as_mut_ptr()).map(|()| into.assume_init()) }
}

#[test]
fn the_header_says_what_this_build_made() {
    assert_eq!(TABLE.header.magic, RESIDENT_MAGIC);
    assert_eq!(TABLE.header.major, RESIDENT_MAJOR);
    assert_eq!(TABLE.header.minor, RESIDENT_MINOR);
    assert_eq!(TABLE.header.size as usize, core::mem::size_of::<ResidentTable>());
}

#[test]
fn the_views_name_the_statics_that_host_tools_peek() {
    let v = real().views;
    assert!(core::ptr::eq(v.state as *const (), addr_of_mut!(crate::state::DKRT_STATE) as *const ()));
    assert!(core::ptr::eq(v.boot_info, addr_of_mut!(crate::boot::DKRT_BOOT)));
    assert!(core::ptr::eq(v.boot_names, addr_of!(crate::boot::BOOT_NAME)));
    assert!(core::ptr::eq(v.paths, addr_of_mut!(crate::state::DKRT_PATHS)));
    assert!(core::ptr::eq(v.menus, addr_of_mut!(crate::state::DKRT_MENUS)));
    assert!(core::ptr::eq(v.menu_objs, addr_of_mut!(crate::state::DKRT_MENU_OBJS)));
    assert!(core::ptr::eq(v.module_runtime, addr_of_mut!(crate::state::DKRT_MODULE_RUNTIME)));
}
