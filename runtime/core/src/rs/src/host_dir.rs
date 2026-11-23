//! The host directory: the owner of `DKRT_HOST_DIR` (`dkrt_abi::host_dir`, ids in `abi.toml`). Host tools read this one symbol to find the statics of the system package,
//! which has none of its own. Stage 1 clears it in the boot, before the package starts (`boot::run`); the package fills it at its entry through the resident table
//! (`resident/host_dir.rs`); the core fills its own ids before it starts the package and marks the directory ready after it; a core-only boot leaves it empty and not ready.

use core::ptr::addr_of_mut;
use dkrt_abi::host_dir::{package_owned, HostDir};
use dkrt_sdk::{HOST_CORE_OWNED, HOST_PACKAGE_HI, HOST_PACKAGE_LO};

#[no_mangle]
pub static mut DKRT_HOST_DIR: HostDir = HostDir::empty();

/// Empty every entry: no package is running.
pub(crate) unsafe fn clear() {
    (*addr_of_mut!(DKRT_HOST_DIR)).clear();
}

/// The package says where its static `id` is. False for an id the package does not own (a core id, or one this core does not know): the entry stays as it is.
pub(crate) unsafe fn publish_package(id: u32, addr: u32) -> bool {
    package_owned(id) && (*addr_of_mut!(DKRT_HOST_DIR)).set(id, addr)
}

/// The value of the core-owned entry `id` for the package placed in `lo..hi`; 0 for an id nobody here knows how to fill (a test of the generated list catches it).
fn core_value(id: u32, lo: u32, hi: u32) -> u32 {
    // if chain, not `match` (a jump table is not ColdFire code)
    if id == HOST_PACKAGE_LO {
        lo
    } else if id == HOST_PACKAGE_HI {
        hi
    } else {
        0
    }
}

/// The core fills every entry it owns, for the package placed in `lo..hi` (before the package starts).
pub(crate) unsafe fn publish_core(lo: u32, hi: u32) {
    let mut i = 0;
    while i < HOST_CORE_OWNED.len() {
        let id = HOST_CORE_OWNED[i];
        (*addr_of_mut!(DKRT_HOST_DIR)).set(id, core_value(id, lo, hi));
        i += 1;
    }
}

/// The package has attached: the directory is whole (written last).
pub(crate) unsafe fn complete() {
    (*addr_of_mut!(DKRT_HOST_DIR)).complete();
}

#[cfg(test)]
mod tests;
