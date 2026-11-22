//! `CardFns`: the card (`storage/card.rs`).

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CardFns {
    /// The store area of the card can be written.
    pub writable: extern "C" fn() -> bool,
}
