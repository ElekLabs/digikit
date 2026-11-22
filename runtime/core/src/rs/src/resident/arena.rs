//! `ArenaFns`: the module arena.
use crate::arena;
use dkrt_abi::dev_addr::DevAddr;
use dkrt_abi::resident::{Group, ArenaFns};

unsafe extern "C" fn alloc(n: u32) -> u32 {
    arena::alloc(n).get()
}
unsafe extern "C" fn free(addr: u32) -> bool {
    arena::free(DevAddr::new(addr))
}
unsafe extern "C" fn stats(out: *mut u32) {
    let (free, largest) = arena::stats();
    *out = free;
    *out.add(1) = largest;
}
extern "C" fn size() -> u32 {
    let s = crate::platform::st();
    s.arena.hi - s.arena.lo
}
extern "C" fn base() -> u32 {
    core::ptr::addr_of!(crate::state::DKRT_ARENA) as usize as u32
}

pub(super) static FNS: Group<ArenaFns> = Group::new(ArenaFns { alloc, free, stats, size, base });
