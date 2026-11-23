//! `ArenaFns`: the module arena (`arena.rs`).

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ArenaFns {
    /// A block of `n` bytes: its device address, 0 when none is free.
    pub alloc: unsafe extern "C" fn(n: u32) -> u32,
    pub free: unsafe extern "C" fn(addr: u32) -> bool,
    /// The free bytes and the largest free piece: `out[0]` and `out[1]`.
    pub stats: unsafe extern "C" fn(out: *mut u32),
    /// The bytes of the arena.
    pub size: extern "C" fn() -> u32,
    /// The device address of the arena.
    pub base: extern "C" fn() -> u32,
}
