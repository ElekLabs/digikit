//! `Gate`: the flags and the queue bookkeeping of an `Owner`, without its types. Every step that must be atomic against a task switch or an interrupt is here, takes the event as
//! bytes and the queue as a pointer, stride and capacity, so one copy of each exists in a service file whatever the owner type, event type and queue length.
use crate::import_core::masked;
use core::cell::Cell;

/// What [`Gate::admit_call`] and [`Gate::admit_event`] decided for one call, in one masked step.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Admit {
    /// The owner was idle and is busy now.
    Enter,
    /// There is no live object (before `start`, after `stop`, during a transition).
    NoOwner,
    /// The owner was inside a call and the event is queued.
    Queued,
    /// The owner was inside a call and there was no room (an event) or the call was not an event (counted).
    Refused,
}

/// The flags and counters of an owner. All-zero bytes at the start, so the owner's cell stays `.bss`. Tested and changed only with interrupts masked.
/// `repr(C)` with the refusal count first: it is the first word of the owner (`DK_SVC_OWNER`), so a device test reads it at the symbol.
#[repr(C)]
pub(super) struct Gate {
    /// Calls and events refused: a nested `with` or `run`, an event that found the queue full.
    refused: Cell<u32>,
    /// Whether the cell holds a live object.
    live: Cell<bool>,
    /// Set while a call into the object runs.
    busy: Cell<bool>,
    /// Set for the whole of `start`'s `init` and of `stop`'s `f` and drop: `start`, `stop` and every call refuse meanwhile, and `live` changes only when the transition ends well.
    transition: Cell<bool>,
    /// The queue: oldest event at `head`, `len` waiting.
    head: Cell<usize>,
    len: Cell<usize>,
}

impl Gate {
    pub(super) const fn new() -> Gate {
        Gate {
            live: Cell::new(false),
            busy: Cell::new(false),
            transition: Cell::new(false),
            head: Cell::new(0),
            len: Cell::new(0),
            refused: Cell::new(0),
        }
    }

    pub(super) fn refused(&self) -> u32 {
        self.refused.get()
    }

    /// Is there a live object, with no transition running?
    #[inline(always)]
    pub(super) fn is_live(&self) -> bool {
        self.live.get() && !self.transition.get()
    }

    /// The decision of a call that is not an event: an idle owner becomes busy; a busy one counts a refusal. One copy for every part function and `with`.
    #[inline(never)]
    pub(super) fn admit_call(&self) -> Admit {
        masked(|| {
            if !self.live.get() || self.transition.get() {
                return Admit::NoOwner;
            }
            if self.busy.get() {
                self.refused.set(self.refused.get() + 1);
                return Admit::Refused;
            }
            self.busy.set(true);
            Admit::Enter
        })
    }

    /// The decision for an event: an idle owner becomes busy; a busy one queues the `stride` bytes at `event` into the queue (`queue`, `cap` slots) when there is room, or counts
    /// a refusal. Only a service with events links this step.
    #[inline(never)]
    pub(super) fn admit_event(
        &self,
        event: *const u8,
        queue: *mut u8,
        stride: usize,
        cap: usize,
    ) -> Admit {
        masked(|| {
            if !self.live.get() || self.transition.get() {
                return Admit::NoOwner;
            }
            if !self.busy.get() {
                self.busy.set(true);
                return Admit::Enter;
            }
            let len = self.len.get();
            if len < cap {
                let mut at = self.head.get() + len;
                if at >= cap {
                    at -= cap;
                }
                // SAFETY: the slot is free (`len < cap`) and `queue` has `cap` slots of `stride` bytes; the queue changes only with interrupts masked.
                unsafe { core::ptr::copy_nonoverlapping(event, queue.add(at * stride), stride) };
                self.len.set(len + 1);
                return Admit::Queued;
            }
            self.refused.set(self.refused.get() + 1);
            Admit::Refused
        })
    }

    /// The end of a call of a service with no events: the owner is idle.
    #[inline(never)]
    pub(super) fn release(&self) {
        masked(|| self.busy.set(false))
    }

    /// The end of a call of a service with events, one masked step: the oldest queued event is copied to `out` (`true`), or the queue is empty and the owner is idle now
    /// (`false`). The last pop and the clearing of the busy flag are the same step, so an event that arrives after the queue was found empty finds the owner idle and runs itself.
    #[inline(never)]
    pub(super) fn pop(&self, queue: *const u8, stride: usize, cap: usize, out: *mut u8) -> bool {
        masked(|| {
            let len = self.len.get();
            if len == 0 {
                self.busy.set(false);
                return false;
            }
            let at = self.head.get();
            // SAFETY: the slot at `head` was written by `admit_event` (`len > 0`); `out` has room for `stride` bytes.
            unsafe { core::ptr::copy_nonoverlapping(queue.add(at * stride), out, stride) };
            self.head.set(if at + 1 == cap { 0 } else { at + 1 });
            self.len.set(len - 1);
            true
        })
    }

    /// Begin `start`: `false` (refused) when the cell holds a live object or a transition runs; otherwise a transition is open.
    #[inline(always)]
    pub(super) fn begin_start(&self) -> bool {
        !masked(|| self.live.get() || self.transition.replace(true))
    }

    /// End `start`: the transition is over; `live` is set in the same masked step when `init` succeeded.
    #[inline(always)]
    pub(super) fn end_start(&self, ok: bool) {
        masked(|| {
            self.transition.set(false);
            if ok {
                self.live.set(true);
            }
        })
    }

    /// Begin `stop`: `false` (refused) while a call runs, when there is no live object, or during a transition; otherwise a transition is open.
    #[inline(always)]
    pub(super) fn begin_stop(&self) -> bool {
        !masked(|| self.busy.get() || !self.live.get() || self.transition.replace(true))
    }

    /// End `stop`: no live object, no transition, in one masked step.
    #[inline(always)]
    pub(super) fn end_stop(&self) {
        masked(|| {
            self.live.set(false);
            self.transition.set(false);
        })
    }
}
