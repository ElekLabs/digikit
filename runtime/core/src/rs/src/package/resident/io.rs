//! The I/O task's queue.
use super::table;

/// The steps of record `rec` are gone: false while one still runs.
#[inline(always)]
pub(crate) unsafe fn retire(rec: usize) -> bool {
    (table().io.retire)(rec as u32)
}
