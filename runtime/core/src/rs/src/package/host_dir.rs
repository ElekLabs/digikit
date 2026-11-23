//! The package's entries of the host directory (`DKRT_HOST_DIR` of the resident core, ids in `abi.toml` `[[host_dir]]`): the addresses of the package statics that host tools
//! read. The package is placed at a different address each boot and has no symbol in the image, so it tells the core where each static is, once, at its entry (`attach`).
//! It fills exactly the ids with `owner = "package"` (the generated `HOST_PACKAGE_OWNED`); the core fills the others. A test walks that list, so an id appended without a row here
//! fails it.

use super::entry::DKRT_TICKS;
use super::{dsp::DKRT_DSP, invariants::DKRT_TABLE_BAD, loader::records::DKRT_PROVIDERS, resident};
use core::ptr::addr_of;
use dkrt_abi::dev_addr::DevAddr;
use dkrt_sdk::{HOST_DSP, HOST_PACKAGE_OWNED, HOST_PROVIDERS, HOST_TABLE_BAD, HOST_TICKS};

/// The device address of the package static `id` stands for; null for an id this package has no static for.
fn address_of(id: u32) -> DevAddr {
    // if chain, not `match` (a jump table is not ColdFire code)
    if id == HOST_DSP {
        DevAddr::of_published(addr_of!(DKRT_DSP))
    } else if id == HOST_PROVIDERS {
        DevAddr::of_published(addr_of!(DKRT_PROVIDERS))
    } else if id == HOST_TICKS {
        DevAddr::of_published(addr_of!(DKRT_TICKS))
    } else if id == HOST_TABLE_BAD {
        DevAddr::of_published(addr_of!(DKRT_TABLE_BAD))
    } else {
        DevAddr::NULL
    }
}

/// Tell the resident core where every package-owned static is (the entry of the package).
pub(crate) unsafe fn publish_all() {
    let mut i = 0;
    while i < HOST_PACKAGE_OWNED.len() {
        let id = HOST_PACKAGE_OWNED[i];
        resident::host_dir::publish(id, address_of(id).get());
        i += 1;
    }
}

#[cfg(test)]
mod tests;
