//! `Group`: one group of the resident table, a size and the entries.

/// A group of the resident table: the bytes the producer built (`size`, the size of its `T`) and then the entries (`T`, a `repr(C)` struct of one owner's pointers).
/// A group only grows at its end, so a package built against a smaller `T` reads the entries it knows and no others: `Resident::open` accepts a group only when
/// its `size` covers the package's whole `T` (the one checked access; no entry is read otherwise).
#[repr(C)]
pub struct Group<T> {
    size: u32,
    pub(super) entries: T,
}

impl<T> Group<T> {
    /// The group of `entries`, with the size of `T`.
    pub const fn new(entries: T) -> Self {
        Group {
            size: core::mem::size_of::<T>() as u32,
            entries,
        }
    }

    /// Bytes of the group the producer built, read without a reference to the whole group: a producer built against an older layout has fewer bytes than this `T`.
    ///
    /// # Safety
    /// `group` points at a live group of a producer that follows the layout rule (the size word comes first).
    pub(super) unsafe fn size_at(group: *const Self) -> u32 {
        core::ptr::addr_of!((*group).size).read()
    }
}
