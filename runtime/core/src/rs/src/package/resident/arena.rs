//! The module arena.
use super::table;
use dkrt_abi::dev_addr::DevAddr;

/// A block of `n` bytes: its device address, `NULL` when none is free.
#[inline(always)]
pub(crate) unsafe fn alloc(n: u32) -> DevAddr {
    DevAddr::new((table().arena.alloc)(n))
}

#[inline(always)]
pub(crate) unsafe fn free(p: DevAddr) -> bool {
    (table().arena.free)(p.get())
}

/// The free bytes and the largest free piece.
#[inline(always)]
pub(crate) unsafe fn stats() -> (u32, u32) {
    let mut out = [0u32; 2];
    (table().arena.stats)(out.as_mut_ptr());
    (out[0], out[1])
}

/// The device address of the arena.
#[inline(always)]
pub(crate) fn base() -> u32 {
    (table().arena.base)()
}
