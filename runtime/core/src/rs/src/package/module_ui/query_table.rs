//! The query table a loaded module registered: `count` entries of a method number and the module's call function. The runtime keeps
//! the module's own table (no copy; it stays valid while the module is loaded) and reads it only on the UI task.

use core::mem::align_of;

/// At most this many methods per module.
pub(super) const QUERIES_MAX: usize = 16;

/// The module's function for one method. `out_len` holds the room in `out` when the call starts and the reply length when it returns 0.
pub(super) type CallFn = unsafe extern "C" fn(*mut u8, *const u8, u32, *mut u8, *mut u32) -> i32;

/// One entry as the module's C code lays it out: `{ uint8_t method; int32_t (*call)(...); }`.
#[repr(C)]
pub(super) struct Query {
    method: u8,
    call: Option<CallFn>,
}

#[derive(Clone, Copy)]
pub(super) struct QueryTable {
    entries: *const Query,
    count: u8,
}

#[cfg(test)]
impl Query {
    pub(super) const fn new(method: u8, call: CallFn) -> Self {
        Self {
            method,
            call: Some(call),
        }
    }

    pub(super) const fn without_function(method: u8) -> Self {
        Self { method, call: None }
    }
}

impl QueryTable {
    pub(super) const EMPTY: Self = Self {
        entries: core::ptr::null(),
        count: 0,
    };

    /// Check a module's table: 1 to `QUERIES_MAX` entries, an aligned pointer, every function set, methods strictly increasing from 1.
    pub(super) unsafe fn parse(table: *const u8, count: u32) -> Option<Self> {
        if table.is_null()
            || table as usize % align_of::<Query>() != 0
            || count == 0
            || count as usize > QUERIES_MAX
        {
            return None;
        }
        let entries = table as *const Query;
        let mut previous = 0u8;
        let mut i = 0;
        while i < count as usize {
            let entry = &*entries.add(i);
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

    /// The function of `method`; `None` for a number the module did not register.
    pub(super) fn find(&self, method: u8) -> Option<CallFn> {
        let mut i = 0;
        while i < self.count as usize {
            // SAFETY: `parse` checked the table, and `withdraw` drops it before the module's memory is released.
            let entry = unsafe { &*self.entries.add(i) };
            if entry.method == method {
                return entry.call;
            }
            i += 1;
        }
        None
    }
}
