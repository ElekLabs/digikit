//! `StoreWriterFns`: the write side of the store (`storage/store_writer.rs`) as the package asks about it.

#[repr(C)]
#[derive(Clone, Copy)]
pub struct StoreWriterFns {
    /// An upload is open.
    pub is_open: extern "C" fn() -> bool,
    /// The store lock is held (a store write is in flight).
    pub is_locked: extern "C" fn() -> bool,
    /// The module with source index `owner` retires: its SDK writes end.
    pub retire: extern "C" fn(owner: u32),
}
