//! The host directory: the owner of `DKRT_HOST_DIR` (`dkrt_abi::host_dir`, ids in `abi.toml`). Host tools read this one symbol to find the statics of the system package,
//! which has none of its own. Stage 1 clears it in the boot, before the package starts (`boot::run`); the package fills it at its entry through the resident table
//! (`resident/host_dir.rs`); a core-only boot leaves it empty.

use core::ptr::addr_of_mut;
use dkrt_abi::host_dir::HostDir;

#[no_mangle]
pub static mut DKRT_HOST_DIR: HostDir = HostDir::empty();

/// Empty every entry: no package is running.
pub(crate) unsafe fn clear() {
    (*addr_of_mut!(DKRT_HOST_DIR)).clear();
}

/// The package says where its static `id` is. False for an id this core does not know (the entry stays as it is).
pub(crate) unsafe fn publish(id: u32, addr: u32) -> bool {
    (*addr_of_mut!(DKRT_HOST_DIR)).set(id, addr)
}

#[cfg(test)]
mod tests;
