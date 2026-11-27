//! Where the store is on this device: the first card sector and the number of sectors, from the device words (`devices/*.toml [card.store]`,
//! `src/cf/<dev>/device.s`). The lowest file of the storage layer: the card gate, the lock and the store all read it.
use core::ptr::{addr_of, read_volatile};
use dkrt_sdk::store::Layout;

extern "C" {
    /// The store area of this device (`devices/*.toml [card.store]`, `src/cf/<dev>/device.s`): the first card sector and the number of sectors.
    static dkrt_store_first: u32;
    static dkrt_store_sectors: u32;
}

/// Host tests: a store layout for this thread (the device words of the host fixture are zero).
#[cfg(test)]
thread_local! {
    pub static TEST_LAYOUT: core::cell::Cell<Option<Layout>> = const { core::cell::Cell::new(None) };
}

/// Where the store is on this device.
pub fn layout() -> Layout {
    #[cfg(test)]
    if let Some(l) = TEST_LAYOUT.with(|c| c.get()) {
        return l;
    }
    unsafe { Layout { first: read_volatile(addr_of!(dkrt_store_first)), sectors: read_volatile(addr_of!(dkrt_store_sectors)) } }
}

