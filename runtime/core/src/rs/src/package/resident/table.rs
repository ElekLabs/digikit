//! The table the resident core gave the package.
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use dkrt_abi::resident::{ResidentTable, RESIDENT_MAGIC, RESIDENT_MAJOR};

/// The table the package calls through: set once at `attach`, null before.
static mut TABLE: *const ResidentTable = core::ptr::null();

/// The package gets the table: every facade function goes through it from now on. Installing the same table again is harmless.
pub(crate) unsafe fn install(table: &'static ResidentTable) {
    assert!(table.header.magic == RESIDENT_MAGIC && table.header.major == RESIDENT_MAJOR);
    write_volatile(addr_of_mut!(TABLE), table);
}

/// The installed table. A call before `attach` is a bug of the caller: the package runs only after the resident core gave it the table.
#[inline(always)]
pub(crate) fn table() -> &'static ResidentTable {
    unsafe { &*read_volatile(addr_of!(TABLE)) }
}
