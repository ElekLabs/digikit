//! The selection index of the module that is loading now (`u32::MAX`: none). The package writes it around every call that runs module or service code or reads the
//! selection, and the services read it through `CoreApi::boot_index` to find the module they belong to.

use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};

static mut BOOT_IDX: u32 = u32::MAX;

/// The index now.
#[inline(always)]
pub(crate) fn get() -> u32 {
    unsafe { read_volatile(addr_of!(BOOT_IDX)) }
}

/// Make `k` the index.
#[inline(always)]
pub(crate) fn set(k: u32) {
    unsafe { write_volatile(addr_of_mut!(BOOT_IDX), k) }
}

/// Run `f` with `k` as the index, and put back the value found.
#[inline(always)]
pub(crate) fn with<R>(k: u32, f: impl FnOnce() -> R) -> R {
    let was = get();
    set(k);
    let r = f();
    set(was);
    r
}
