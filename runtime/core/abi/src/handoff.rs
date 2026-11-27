//! `Handoff`: the one-word phase that hands a cell of data from one task to another (the UI task and the I/O or job task of a service, `services/project` `StoreJob`,
//! `services/samples` `PeakJob`). One side owns the data in some phases and the other side in the rest; a phase is published by one aligned word store, after the data of
//! that side is complete.
//!
//! The word is accessed volatile, and a compiler fence orders the data around it: [`Handoff::publish`] puts a release fence before the store (no store of the data moves
//! past the phase), [`Handoff::phase`] puts an acquire fence after the load (no access of the data moves before the phase was read). The ColdFire has one core and no
//! reordering of its own, so the fences only bind the compiler. All-zero bytes are the phase 0, so a cell that holds one is `.bss`.
use core::cell::UnsafeCell;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{compiler_fence, Ordering};

/// The phase word of a cell.
pub struct Handoff {
    word: UnsafeCell<u32>,
}

// One aligned word, read and written volatile; the data it guards is the user's to hand over (the user's `Sync` impl says which side touches it when).
unsafe impl Sync for Handoff {}

impl Handoff {
    /// A cell in phase 0 (all-zero bytes).
    pub const fn new() -> Handoff {
        Handoff {
            word: UnsafeCell::new(0),
        }
    }

    /// The phase now. Accesses to the data that follow are not moved before this read.
    pub fn phase(&self) -> u32 {
        // SAFETY: an aligned word of the cell, written by one store
        let p = unsafe { read_volatile(self.word.get()) };
        compiler_fence(Ordering::Acquire);
        p
    }

    /// Give the data over: every write to it that precedes this call is complete when the new phase is visible.
    pub fn publish(&self, phase: u32) {
        compiler_fence(Ordering::Release);
        // SAFETY: as in `phase`
        unsafe { write_volatile(self.word.get(), phase) }
    }
}

impl Default for Handoff {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
