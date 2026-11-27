//! The exports the menu and the modules call, where the package has to tell them something.
use super::table;

/// The card is busy with a firmware operation.
#[inline(always)]
pub(crate) unsafe fn card_busy() -> bool {
    (table().api.card_busy)()
}

/// Module record `owner` is unloaded; its code lay at `base`, `size` bytes.
#[inline(always)]
pub(crate) unsafe fn notify_unload(owner: usize, base: u32, size: u32) {
    (table().api.notify_unload)(owner as u32, base, size)
}

#[inline(always)]
pub(crate) unsafe fn poll_drop_range(base: u32, size: u32) {
    (table().api.poll_drop_range)(base, size)
}
