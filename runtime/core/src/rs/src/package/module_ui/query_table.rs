//! The query table a loaded module registered: `count` entries of a method number and the module's call function. The runtime keeps
//! the module's own table (no copy; it stays valid while the module is loaded) and reads it only on the UI task.

use core::mem::size_of;
use core::ptr::read_unaligned;

/// At most this many methods per module.
pub(crate) const QUERIES_MAX: usize = 16;

/// The entry as a module's code holds it (ColdFire, 32-bit pointers): its size and the place of the method number. A host cannot read the packed
/// bytes through `Query` (its pointer is wider there), so the assembly tests read them by these numbers; the assert ties them to `Query`.
pub(crate) const WIRE_ENTRY_SIZE: usize = 8;
pub(crate) const WIRE_METHOD_OFFSET: usize = 4;

/// The module's function for one method. `out_len` holds the room in `out` when the call starts and the reply length when it returns 0.
pub(super) type CallFn = unsafe extern "C" fn(*mut u8, *const u8, u32, *mut u8, *mut u32) -> i32;

/// One entry as `<dk/query.h>` lays it out: `{ call; uint8_t method; uint8_t reserved[3]; }`, 8 bytes in the module's code. The ColdFire C
/// compiler aligns pointers to 2 bytes, so a table may start on any 2-byte boundary and entries are read unaligned.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct Query {
    call: Option<CallFn>,
    method: u8,
    reserved: [u8; 3],
}

#[cfg(target_pointer_width = "32")]
const _: () = assert!(size_of::<Query>() == WIRE_ENTRY_SIZE && core::mem::offset_of!(Query, method) == WIRE_METHOD_OFFSET);

#[derive(Clone, Copy)]
pub(super) struct QueryTable {
    entries: *const Query,
    count: u8,
}

#[cfg(test)]
impl Query {
    pub(super) const fn new(method: u8, call: CallFn) -> Self {
        Self {
            call: Some(call),
            method,
            reserved: [0; 3],
        }
    }

    pub(super) const fn without_function(method: u8) -> Self {
        Self {
            call: None,
            method,
            reserved: [0; 3],
        }
    }
}

impl QueryTable {
    pub(super) const EMPTY: Self = Self {
        entries: core::ptr::null(),
        count: 0,
    };

    /// Check a module's table: 1 to `QUERIES_MAX` entries, a 2-byte aligned pointer, every function set, methods strictly increasing from 1.
    pub(super) unsafe fn parse(table: *const u8, count: u32) -> Option<Self> {
        if table.is_null() || table as usize % 2 != 0 || count == 0 || count as usize > QUERIES_MAX
        {
            return None;
        }
        let entries = table as *const Query;
        let mut previous = 0u8;
        let mut i = 0;
        while i < count as usize {
            let entry = read_unaligned(entries.add(i));
            if entry.method <= previous || entry.call.is_none() {
                return None;
            }
            previous = entry.method;
            i += 1;
        }
        Some(Self {
            entries,
            count: count as u8,
        })
    }

    pub(super) fn is_empty(&self) -> bool {
        self.count == 0
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.count as usize
    }

    /// The function of `method`; `None` for a number the module did not register.
    pub(super) fn find(&self, method: u8) -> Option<CallFn> {
        let mut i = 0;
        while i < self.count as usize {
            // SAFETY: `parse` checked the table, and `withdraw` drops it before the module's memory is released.
            let entry = unsafe { read_unaligned(self.entries.add(i)) };
            if entry.method == method {
                return entry.call;
            }
            i += 1;
        }
        None
    }
}
