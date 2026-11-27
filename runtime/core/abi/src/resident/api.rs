//! `ApiFns`: the exports the menu and the modules call (`api.rs`), where the package has to tell them something.

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ApiFns {
    /// The card is busy with a firmware operation.
    pub card_busy: unsafe extern "C" fn() -> bool,
    /// Module record `owner` is unloaded; its code lay at `base`, `size` bytes.
    pub notify_unload: unsafe extern "C" fn(owner: u32, base: u32, size: u32),
    /// Drop the poll functions whose address lies in `base..base + size`.
    pub poll_drop_range: unsafe extern "C" fn(base: u32, size: u32),
}
