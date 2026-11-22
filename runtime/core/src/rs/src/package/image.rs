//! The entry of the system package artifact (`--features package-image`): the package and nothing of the resident core. The package is position-independent
//! only through its relocations (`patchkit syspkg build` writes them); it reaches the resident core through the table it is given and nothing else.
//!
//! Stage 1 places and relocates the body, then calls `dkpkg_entry` with the resident table. The entry checks the table (`dkrt_abi::resident::check`) and returns
//! the package's hooks, or null when the table does not fit; the resident core installs the hooks.

use super::{attach, resident};
use core::panic::PanicInfo;
use dkrt_abi::hooks::PackageHooks;
use dkrt_abi::resident::{ResidentTable, RESIDENT_MAJOR, RESIDENT_MINOR};

/// The resident table version this package was built against, `major << 16 | minor`: `patchkit syspkg build` writes it into the package header, where stage 1 checks it
/// before it places the body (`dkrt_sdk::syspkg::Header::compatible`). The version has this one source.
#[no_mangle]
pub static DKPKG_TABLE: u32 = (RESIDENT_MAJOR as u32) << 16 | RESIDENT_MINOR as u32;

/// The one symbol the artifact exports; `patchkit syspkg build` reads its offset into the body header.
#[no_mangle]
pub unsafe extern "C" fn dkpkg_entry(table: *const ResidentTable) -> *const PackageHooks {
    match attach(&*table) {
        Ok(hooks) => hooks,
        Err(_) => core::ptr::null(),
    }
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    resident::fault::panic()
}
