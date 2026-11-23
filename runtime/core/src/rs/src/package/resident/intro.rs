//! The boot screen.
use super::table;

#[inline(always)]
pub(crate) unsafe fn commit(rec: usize) {
    (table().intro.commit)(rec as u32)
}

#[inline(always)]
pub(crate) unsafe fn discard(rec: usize) {
    (table().intro.discard)(rec as u32)
}
