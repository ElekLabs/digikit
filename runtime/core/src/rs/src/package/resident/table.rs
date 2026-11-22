//! The table the resident core gave the package: checked once, read through the groups.
use core::mem::MaybeUninit;
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use dkrt_abi::resident::{check, Refusal, Resident, ResidentTable, RESIDENT_MINOR};

/// The groups of the accepted table (`dkrt_abi::resident::check`), copied: written once at `attach`, read through `TABLE`.
static mut GROUPS: MaybeUninit<Resident> = MaybeUninit::uninit();

/// The groups the package calls through: set once at `attach`, null before.
static mut TABLE: *const Resident = core::ptr::null();

/// The package gets the table: it is checked against the version this build knows (`RESIDENT_MINOR`, the layout of every group) and, when it fits, every facade
/// function goes through it from now on. A table that does not fit gives the reason and changes nothing. Installing the same table again is harmless.
pub(crate) unsafe fn install(table: &'static ResidentTable) -> Result<(), Refusal> {
    // an accepted table fills `GROUPS` (a refused one leaves it as it was)
    check(table, RESIDENT_MINOR, addr_of_mut!(GROUPS) as *mut Resident)?;
    write_volatile(addr_of_mut!(TABLE), addr_of!(GROUPS) as *const Resident);
    Ok(())
}

/// The installed groups. A call before `attach` is a bug of the caller: the package runs only after the resident core gave it the table.
#[inline(always)]
pub(crate) fn table() -> &'static Resident {
    unsafe { &*read_volatile(addr_of!(TABLE)) }
}
