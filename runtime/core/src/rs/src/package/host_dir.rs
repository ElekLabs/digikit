//! The package's entries of the host directory (`DKRT_HOST_DIR` of the resident core, ids in `abi.toml` `[[host_dir]]`): the addresses of the package statics that host tools
//! read. The package is placed at a different address each boot and has no symbol in the image, so it tells the core where each static is, once, at its entry (`attach`).
//! One row per id, indexed by the id: `addresses` has no entry left unset (a test checks that every id of `abi.toml` has one).

use super::entry::DKRT_TICKS;
use super::{dsp::DKRT_DSP, invariants::DKRT_TABLE_BAD, loader::records::DKRT_PROVIDERS, resident};
use core::ptr::addr_of;
use dkrt_abi::dev_addr::DevAddr;
use dkrt_sdk::{HOST_COUNT, HOST_DSP, HOST_PROVIDERS, HOST_TABLE_BAD, HOST_TICKS};

/// The device address of each package static a tool reads, by id.
fn addresses() -> [DevAddr; HOST_COUNT] {
    let mut rows = [DevAddr::NULL; HOST_COUNT];
    rows[HOST_DSP as usize] = DevAddr::of_published(addr_of!(DKRT_DSP));
    rows[HOST_PROVIDERS as usize] = DevAddr::of_published(addr_of!(DKRT_PROVIDERS));
    rows[HOST_TICKS as usize] = DevAddr::of_published(addr_of!(DKRT_TICKS));
    rows[HOST_TABLE_BAD as usize] = DevAddr::of_published(addr_of!(DKRT_TABLE_BAD));
    rows
}

/// Tell the resident core where every static is (the entry of the package).
pub(crate) unsafe fn publish_all() {
    let rows = addresses();
    let mut id = 0;
    while id < HOST_COUNT {
        resident::host_dir::publish(id as u32, rows[id].get());
        id += 1;
    }
}

#[cfg(test)]
mod tests;
