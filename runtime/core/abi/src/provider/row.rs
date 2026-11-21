use super::Status;
use core::mem::{align_of, size_of};
use core::ops::Range;
use dkrt_sdk::E_FORMAT;

/// Changes only when [`ProviderRow`] or [`PartRef`] changes.
pub const PROVIDER_ABI: u32 = 1;

/// The lifecycle part (required): the function table of `Provider`.
pub const PART_LIFECYCLE: u32 = 1;
pub const PART_RECORDS: u32 = 2;
pub const PART_DSP: u32 = 3;
pub const PART_TICK: u32 = 4;
pub const PART_QUERY: u32 = 5;
pub const PART_DESCRIBE: u32 = 6;

/// One part of a service: its tag, its version (a new callback is a new version of one part) and its function table.
///
/// Layout, the same on every 32-bit target (ColdFire aligns `u32` and pointers to 2 bytes): 12 bytes, no implicit padding. The host has 64-bit pointers
/// and a 16-byte entry; only the 32-bit layout is the card ABI (asserted below).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PartRef {
    tag: u32,
    version: u16,
    reserved: u16,
    table: *const (),
}

// A part table is immutable static data.
unsafe impl Sync for PartRef {}

impl PartRef {
    pub const fn new(tag: u32, version: u16, table: *const ()) -> PartRef {
        PartRef {
            tag,
            version,
            reserved: 0,
            table,
        }
    }
}

/// The row a service file exports: its name hash and its parts. Plain data: nothing here dereferences `parts`. A row read from a file is read only
/// through [`check`], which returns a [`CheckedRow`].
///
/// Layout on 32-bit targets: 16 bytes, no implicit padding (asserted below).
#[repr(C)]
#[derive(Debug)]
pub struct ProviderRow {
    abi: u32,
    name: u32,
    parts: *const PartRef,
    nparts: u16,
    reserved: u16,
}

unsafe impl Sync for ProviderRow {}

#[cfg(target_pointer_width = "32")]
const _: () = {
    use core::mem::offset_of;
    assert!(size_of::<PartRef>() == 12);
    assert!(offset_of!(PartRef, tag) == 0);
    assert!(offset_of!(PartRef, version) == 4);
    assert!(offset_of!(PartRef, reserved) == 6);
    assert!(offset_of!(PartRef, table) == 8);
    assert!(size_of::<ProviderRow>() == 16);
    assert!(offset_of!(ProviderRow, abi) == 0);
    assert!(offset_of!(ProviderRow, name) == 4);
    assert!(offset_of!(ProviderRow, parts) == 8);
    assert!(offset_of!(ProviderRow, nparts) == 12);
    assert!(offset_of!(ProviderRow, reserved) == 14);
};

impl ProviderRow {
    /// A row over `parts`. More than `u16::MAX` parts is refused (a panic, a compile error in a `static`), never truncated.
    pub const fn new(name: u32, parts: &'static [PartRef]) -> ProviderRow {
        assert!(parts.len() <= u16::MAX as usize);
        ProviderRow {
            abi: PROVIDER_ABI,
            name,
            parts: parts.as_ptr(),
            nparts: parts.len() as u16,
            reserved: 0,
        }
    }

    pub fn abi(&self) -> u32 {
        self.abi
    }

    pub fn name(&self) -> u32 {
        self.name
    }
}

/// The answer of [`CheckedRow::part`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartLookup {
    /// The part is there at a version the core knows.
    Found { table: *const (), version: u16 },
    /// The service did not adopt the part.
    Missing,
    /// The part's version is newer than the core knows: admission refuses the service.
    TooNew { version: u16 },
}

/// A row whose shape [`check`] validated: the part list is inside the package, free of duplicate tags and holds the lifecycle part.
///
/// The lifetime `'a` is the borrow of the `&ProviderRow` given to [`check`], not of the package: the compiler cannot see when the package image is
/// freed. The part tables a row points to are valid from link until the package is freed, so the caller must drop every `CheckedRow` (and every
/// table pointer taken from one) before it frees the package image.
#[derive(Clone, Copy, Debug)]
pub struct CheckedRow<'a> {
    name: u32,
    parts: &'a [PartRef],
}

impl CheckedRow<'_> {
    pub fn name(&self) -> u32 {
        self.name
    }

    /// Find the part with `tag`. `newest` is the newest version of that part the core knows.
    pub fn part(&self, tag: u32, newest: u16) -> PartLookup {
        match self.parts.iter().find(|p| p.tag == tag) {
            None => PartLookup::Missing,
            Some(p) if p.version > newest => PartLookup::TooNew { version: p.version },
            Some(p) => PartLookup::Found {
                table: p.table,
                version: p.version,
            },
        }
    }
}

/// Admission check of a row read from a file. `extent` is the address range of the package the row lives in. `E_FORMAT` for another `abi`, a row or
/// part list outside `extent` (or null or misaligned), a duplicate tag or no lifecycle part.
///
/// The returned [`CheckedRow`] borrows the `&ProviderRow`, not the package: nothing ties it to the package image's lifetime. This is why `check` is
/// `unsafe`: the caller must drop every `CheckedRow` before the package image is freed.
///
/// # Safety
///
/// Every byte of `extent` must be readable and initialised, and stay so while the returned view is alive. The core calls this after link and drops
/// the view before it frees the package.
pub unsafe fn check(row: &ProviderRow, extent: Range<usize>) -> Result<CheckedRow<'_>, Status> {
    let inside = |at: usize, len: usize| {
        at.checked_add(len)
            .is_some_and(|end| at >= extent.start && end <= extent.end)
    };
    if row.abi != PROVIDER_ABI
        || !inside(row as *const ProviderRow as usize, size_of::<ProviderRow>())
    {
        return Err(E_FORMAT);
    }
    let at = row.parts as usize;
    let len = (row.nparts as usize) * size_of::<PartRef>();
    if row.parts.is_null() || at % align_of::<PartRef>() != 0 || !inside(at, len) {
        return Err(E_FORMAT);
    }
    // SAFETY: non-null, aligned, `len` bytes inside `extent`, which the caller guarantees readable.
    let parts = unsafe { core::slice::from_raw_parts(row.parts, row.nparts as usize) };
    for (i, p) in parts.iter().enumerate() {
        if parts[..i].iter().any(|q| q.tag == p.tag) {
            return Err(E_FORMAT);
        }
    }
    if !parts.iter().any(|p| p.tag == PART_LIFECYCLE) {
        return Err(E_FORMAT);
    }
    Ok(CheckedRow {
        name: row.name,
        parts,
    })
}

#[cfg(test)]
mod tests;
