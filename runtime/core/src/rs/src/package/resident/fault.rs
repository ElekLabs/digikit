//! The crash capture.
use super::table;

/// A panic: write the crash record and stop (the next boot is a safe one).
#[inline(always)]
pub(crate) fn panic() -> ! {
    (table().fault.panic)()
}
