//! The write side of the store, as the package asks about it.
use super::table;

/// An upload is open.
#[inline(always)]
pub(crate) fn is_open() -> bool {
    (table().store_writer.is_open)()
}

/// The store lock is held (a store write is in flight).
#[inline(always)]
pub(crate) fn is_locked() -> bool {
    (table().store_writer.is_locked)()
}

/// The module with source index `owner` retires: its SDK writes end.
#[inline(always)]
pub(crate) fn retire(owner: u8) {
    (table().store_writer.retire)(owner as u32)
}
