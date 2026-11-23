//! `Owner<T>`: the one cell of a service file that holds its owner object (`Provider`), made by `start` in the file's `init` and taken back by `stop` in `fini`.
use crate::ImportCore;
use core::cell::{Cell, UnsafeCell};
use core::mem::MaybeUninit;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::provider::Status;
use dkrt_sdk::E_LOADED;

/// An event type with no value: the event of an owner that takes no events ([`Owner::post`] cannot be called).
#[derive(Clone, Copy)]
pub enum NoEvent {}

/// What an owner does with an event: the way the object takes one. A zero-size type (the `A` of [`Owner`]), so the cell holds no function pointer and stays all-zero
/// (`.bss`); `service!` makes it from the service's `Reentry` part.
pub trait Events<T> {
    /// One event: plain data, copied into the queue.
    type Event: Copy;
    /// Run `event` on the object, with the owner busy and no other reference to it.
    fn apply(owner: &mut T, event: Self::Event);
}

/// The events of an owner that takes none.
pub struct NoEvents;

impl<T> Events<T> for NoEvents {
    type Event = NoEvent;
    fn apply(_: &mut T, event: NoEvent) {
        match event {}
    }
}

/// What [`Owner::post`] did with an event.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Post {
    /// The owner was idle: the event ran at once.
    Ran,
    /// The owner was inside a call: the event is queued and runs right after that call returns, before the owner is idle again.
    Queued,
    /// The owner was inside a call and the queue is full: the event is refused and counted ([`Owner::refused`]).
    Full,
    /// There is no live owner (before `start`, after `stop`): the event is dropped.
    NoOwner,
}

/// What `Owner::admit` decided for one call, taken in one masked step.
enum Admit<T> {
    /// The owner was idle and is busy now: the object's address.
    Enter(*mut T),
    /// There is no live object (before `start`, after `stop`, during a transition).
    NoOwner,
    /// The owner was inside a call and the event is queued.
    Queued,
    /// The owner was inside a call and there was no room (the event) or the call was not an event (counted in `refused`).
    Refused,
}

/// The cell. A file is one service and has one; `service!` declares it as `DK_SVC_OWNER`, so the file's hooks reach the instance with [`Owner::with`] and the part
/// functions through the guard `service!` gives their tables ([`Owner::enter`], [`Owner::leave`]). The UI task, the job task and interrupts may all call in.
///
/// Re-entry: every call into the object (`with`, `run`, `post`) marks the owner busy until it returns. The test of the busy flag and its change, and every change of the queue,
/// are made with interrupts masked (`dkc_irq_mask`, the import `lfo` uses for its shape table), so a task switch or an interrupt cannot fall between the test and the set: two
/// calls never both find the owner idle. A call that arrives meanwhile (a callback of the same service, a hook in an interrupt, the other task) never forms a second `&mut T`:
/// `with` and `run` refuse it, and [`Owner::post`] queues a typed event (`A::Event`, at most `N` waiting, no heap) that runs through `A::apply` right after the outer call
/// returns, before the owner is idle. A hook that must not lose its input uses [`Owner::with_or_post`]. A refused call or a full queue is counted in [`Owner::refused`],
/// never dropped silently. The object itself is never shared: state that a job in another task reads or writes lives in a cell of its own, outside `T`.
pub struct Owner<T, A: Events<T> = NoEvents, const N: usize = 0> {
    /// The state, uninitialised until `start`: all-zero bytes, so the cell is `.bss` and costs no file bytes.
    cell: UnsafeCell<MaybeUninit<T>>,
    /// Whether `cell` holds a live `T`.
    live: Cell<bool>,
    /// Set while a call into the object runs; tested and set only with interrupts masked.
    busy: Cell<bool>,
    /// Set for the whole of `start`'s `init` and of `stop`'s `f` and drop: `start`, `stop` and every call refuse meanwhile, and `live` changes only when the transition ends well.
    transition: Cell<bool>,
    /// The events that arrived while busy, oldest at `head`; all-zero bytes at the start.
    queue: UnsafeCell<[MaybeUninit<A::Event>; N]>,
    head: Cell<usize>,
    len: Cell<usize>,
    /// Calls and events refused: a nested `with` or `run`, an event that found the queue full.
    refused: Cell<u32>,
}

// The cells are changed only with interrupts masked (the flags and the queue) or by the one call that holds the busy flag (the object).
unsafe impl<T, A: Events<T>, const N: usize> Sync for Owner<T, A, N> {}

/// Run `f` with interrupts masked (a single core: nothing else runs meanwhile).
#[inline(always)]
fn masked<R>(f: impl FnOnce() -> R) -> R {
    let sr = ImportCore.irq_mask();
    let r = f();
    ImportCore.irq_restore(sr);
    r
}

impl<T, A: Events<T>, const N: usize> Owner<T, A, N> {
    pub const fn new() -> Owner<T, A, N> {
        Owner {
            cell: UnsafeCell::new(MaybeUninit::zeroed()),
            live: Cell::new(false),
            busy: Cell::new(false),
            transition: Cell::new(false),
            queue: UnsafeCell::new([const { MaybeUninit::zeroed() }; N]),
            head: Cell::new(0),
            len: Cell::new(0),
            refused: Cell::new(0),
        }
    }

    /// How many calls and events were refused (a nested `with` or `run`, a full queue) since the cell was made.
    pub fn refused(&self) -> u32 {
        self.refused.get()
    }

    /// Make the instance in the cell with `init`, which writes the whole value into the slot it gets (no copy of the state through the stack) and keep it; the pointer to it
    /// is what the row's part functions work on. `E_LOADED` when the cell holds one already (also inside a `with` closure, which needs a live value): `init` is not called. When `init` fails the cell stays empty (it must not
    /// have left anything that needs a drop).
    pub fn start(
        &self,
        init: impl FnOnce(&mut MaybeUninit<T>) -> Result<(), Status>,
    ) -> Result<*mut T, Status> {
        if masked(|| self.live.get() || self.transition.replace(true)) {
            return Err(E_LOADED);
        }
        // SAFETY: the cell holds no live value and `transition` keeps every call out, so no reference into it is alive here.
        let r = init(unsafe { &mut *self.cell.get() });
        masked(|| self.transition.set(false));
        r?;
        self.live.set(true);
        Ok(self.cell.get() as *mut T)
    }

    /// The instance for the duration of `f`; `None` before `start`, after `stop` and inside another call into the same owner (counted in `refused`).
    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        self.run(None, |t| Some(f(t)))
    }

    /// `f` on the instance as one call: the owner is busy until it returns, then the events that arrived meanwhile run, in order. `refused` is the answer, and `f` is
    /// not called, before `start`, after `stop` and inside another call into the same owner (the latter counted in `refused()`).
    pub fn run<R>(&self, refused: R, f: impl FnOnce(&mut T) -> R) -> R {
        let at = self.enter();
        if at.is_null() {
            return refused;
        }
        // SAFETY: `enter` made the owner busy, so this is the only reference, and `live` says the cell holds a value.
        let r = f(unsafe { &mut *at });
        self.leave();
        r
    }

    /// `f` on the instance when the owner is idle; inside a call, `event` is queued instead (it runs when that call returns) and the answer says what became of it. For a hook
    /// that may fire while the service is inside a call, in the same task or from an interrupt, and whose input must not be lost.
    pub fn with_or_post<R>(&self, event: A::Event, f: impl FnOnce(&mut T) -> R) -> Result<R, Post> {
        match self.admit(Some(event)) {
            Admit::Enter(at) => {
                // SAFETY: as in `run`.
                let r = f(unsafe { &mut *at });
                self.leave();
                Ok(r)
            }
            Admit::NoOwner => Err(Post::NoOwner),
            Admit::Queued => Err(Post::Queued),
            Admit::Refused => Err(Post::Full),
        }
    }

    /// The one decision of every call, with interrupts masked: an idle owner becomes busy; a busy one queues `event` (when there is one, and room) or counts a refusal.
    #[inline(never)]
    fn admit(&self, event: Option<A::Event>) -> Admit<T> {
        masked(|| {
            if !self.live.get() || self.transition.get() {
                return Admit::NoOwner;
            }
            if !self.busy.get() {
                self.busy.set(true);
                return Admit::Enter(self.cell.get() as *mut T);
            }
            if let Some(event) = event {
                let len = self.len.get();
                if len < N {
                    // SAFETY: the slot is free (`len < N`); the queue changes only with interrupts masked.
                    unsafe { (*self.queue.get())[(self.head.get() + len) % N].write(event) };
                    self.len.set(len + 1);
                    return Admit::Queued;
                }
            }
            self.refused.set(self.refused.get() + 1);
            Admit::Refused
        })
    }

    /// Start of a call (the guard of the part functions of a table): the object's address and the owner is busy, or null (nothing changes but the count) when there is no live
    /// object or a call runs already. One copy for every part function; the caller gives the refusal answer and calls [`Owner::leave`] when the call ends.
    #[inline(never)]
    pub fn enter(&self) -> *mut T {
        match self.admit(None) {
            Admit::Enter(at) => at,
            _ => core::ptr::null_mut(),
        }
    }

    /// Hand `event` to the object. When the owner is idle it runs at once; inside a call it is queued, to run right after that call returns (see the type).
    pub fn post(&self, event: A::Event) -> Post {
        match self.admit(Some(event)) {
            Admit::Enter(at) => {
                // SAFETY: as in `run`.
                A::apply(unsafe { &mut *at }, event);
                self.leave();
                Post::Ran
            }
            Admit::NoOwner => Post::NoOwner,
            Admit::Queued => Post::Queued,
            Admit::Refused => Post::Full,
        }
    }

    /// End of a call: run the queued events in order (each may queue more), then the owner is idle. The last pop and the clearing of the busy flag are one masked step, so an
    /// event that arrives after the queue was found empty finds the owner idle and runs itself.
    #[inline(never)]
    pub fn leave(&self) {
        loop {
            let next = masked(|| {
                let len = self.len.get();
                if len == 0 {
                    self.busy.set(false);
                    return None;
                }
                let at = self.head.get();
                // SAFETY: the slot at `head` was written by `admit` (`len > 0`).
                let event = unsafe { (*self.queue.get())[at].assume_init_read() };
                self.head.set((at + 1) % N);
                self.len.set(len - 1);
                Some(event)
            });
            let Some(event) = next else { return };
            // SAFETY: still busy, so no other reference exists.
            A::apply(unsafe { &mut *(self.cell.get() as *mut T) }, event);
        }
    }

    /// The address of the instance, null when there is none. It is for the row's instance word and for the check that an owner is live: nothing may form a reference through it
    /// (a call goes through [`Owner::run`] or [`Owner::enter`], which refuse a second `&mut T`).
    pub fn instance(&self) -> *mut T {
        if self.live.get() && !self.transition.get() {
            self.cell.get() as *mut T
        } else {
            core::ptr::null_mut()
        }
    }

    /// End the instance: `f` works on it in place (the file's `fini` gives it `Provider::stop`), then it is dropped in place. A hook that fires inside `f` finds none
    /// (every call refuses during the transition) and cannot start or stop. `false` when there was none, and while a call runs (it holds `&mut T`: the state stays intact).
    pub fn stop(&self, f: impl FnOnce(&mut T)) -> bool {
        if masked(|| self.busy.get() || !self.live.get() || self.transition.replace(true)) {
            return false;
        }
        let at = self.cell.get() as *mut T;
        // SAFETY: `live` was set, so the cell holds a value; `transition` keeps `start`, `stop` and every call out until it is dropped, once.
        unsafe {
            f(&mut *at);
            core::ptr::drop_in_place(at);
        }
        self.live.set(false);
        self.transition.set(false);
        true
    }
}

impl<T, A: Events<T>, const N: usize> Default for Owner<T, A, N> {
    fn default() -> Self {
        Owner::new()
    }
}

#[cfg(test)]
mod tests;
