//! `Shared<T>`: a cell for a small table that several tasks and interrupts read and change, kept apart from any owner object.
//!
//! The ColdFire runs one core, so a section with interrupts masked (`CoreApi::irq_mask`) is atomic against every task and interrupt: nothing else runs in it. [`Shared::with`]
//! runs a closure in such a section and gives it the one `&mut T`. This is the cell for state that a call must reach at once from any task, while an owner object (`Owner<T>` of
//! `dkrt-svc-rt`) may be busy in another call of the same task or in a task that was preempted: the table lives here, never as a field of the owner.
//!
//! Rules for a section (the closure):
//! - It is short and bounded: a fixed number of steps over fixed-size data (a table scan, a copy of a few hundred bytes). Interrupts are late by its length.
//! - No I/O, no call out of the crate, no allocation, no waiting, no loop that depends on another task.
//! - It does not call `with` on the same cell (a second `&mut T`): that is a bug and panics.
//! - Heavy work does not run in it: the owner reads what it needs in one short section, works outside, and writes the result back in another.
//!
//! All-zero bytes of `T` are the initial state of a cell made with a zeroed `T`, so a static of it is `.bss`.
use crate::core_api::CoreApi;
use core::cell::{Cell, UnsafeCell};

/// A table changed only with interrupts masked. See the module.
pub struct Shared<T> {
    value: UnsafeCell<T>,
    /// Set while a section runs: a nested `with` is a second `&mut T`.
    held: Cell<bool>,
}

// The value and the flag are touched only inside a masked section, which no other task or interrupt can enter on the single core.
unsafe impl<T> Sync for Shared<T> {}

impl<T> Shared<T> {
    pub const fn new(value: T) -> Shared<T> {
        Shared {
            value: UnsafeCell::new(value),
            held: Cell::new(false),
        }
    }

    /// `f` on the value with interrupts masked (`core.irq_mask()`, undone by `core.irq_restore` when `f` returns). See the module for what `f` may do.
    ///
    /// Panics when called inside a section of the same cell.
    pub fn with<C: CoreApi, R>(&self, core: &C, f: impl FnOnce(&mut T) -> R) -> R {
        let sr = core.irq_mask();
        assert!(!self.held.get(), "nested section on one Shared");
        self.held.set(true);
        // SAFETY: interrupts are masked and `held` was clear, so no other reference to the value exists.
        let r = f(unsafe { &mut *self.value.get() });
        self.held.set(false);
        core.irq_restore(sr);
        r
    }

    /// Replace the value with no mask: for the reset of a host test or a start before any other task runs. Not a section: nothing else may touch the cell meanwhile.
    ///
    /// # Safety
    /// No task or interrupt uses the cell while this runs.
    pub unsafe fn set_unmasked(&self, value: T) {
        assert!(!self.held.get(), "set inside a section of the same Shared");
        *self.value.get() = value;
    }
}
