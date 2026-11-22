//! The card.
use super::table;

/// The store area of the card can be written.
#[inline(always)]
pub(crate) fn writable() -> bool {
    (table().card.writable)()
}
