//! `Owner<T>`: the one cell of a service file that holds its owner object (`Provider`), made by `start` in the file's `init` and taken back by `stop` in `fini`.
use core::cell::{Cell, UnsafeCell};
use core::mem::MaybeUninit;
use dkrt_abi::provider::Status;
use dkrt_sdk::E_LOADED;

/// An event type with no value: the `E` of an owner that takes no events ([`Owner::post`] cannot be called).
#[derive(Clone, Copy)]
pub enum NoEvent {}

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

/// The cell. A file is one service and has one; `service!` declares it as `DK_SVC_OWNER`, so the file's hooks reach the instance with [`Owner::with`] and the part
/// functions through the guard `service!` gives their tables ([`Owner::run`]). Every call runs in the UI task (or in an interrupt that the service guards itself), never
/// concurrently; the I/O task and interrupts never go through the owner.
///
/// Re-entry: every call into the object (`with`, `run`, `post`) marks the owner busy until it returns. A call that arrives meanwhile (a callback of the same service,
/// fired by a call the object made) never forms a second `&mut T`: `with` and `run` refuse it, and [`Owner::post`] queues a typed event (`E`, at most `N` waiting,
/// no heap) that runs through the owner's `apply` right after the outer call returns, in the same task, before the owner is idle. A refused call or a full queue is
/// counted in [`Owner::refused`], never dropped silently.
pub struct Owner<T, E: Copy = NoEvent, const N: usize = 0> {
    /// The state, uninitialised until `start`: all-zero bytes, so the cell is `.bss` and costs no file bytes.
    cell: UnsafeCell<MaybeUninit<T>>,
    /// Whether `cell` holds a live `T`.
    live: Cell<bool>,
    /// Set while `with` runs a closure: a nested `with` (a hook that fires inside a part function of the same owner) finds nothing, instead of a second `&mut T`.
    busy: Cell<bool>,
    /// Set for the whole of `start`'s `init` and of `stop`'s `f` and drop: `start`, `stop` and `with` refuse meanwhile, and `live` changes only when the transition ends well.
    transition: Cell<bool>,
    /// How an event of the queue reaches the object (`Reentry::apply`); none for an owner without events.
    apply: Option<fn(&mut T, E)>,
    /// The events that arrived while busy, oldest at `head`.
    queue: [Cell<Option<E>>; N],
    head: Cell<usize>,
    len: Cell<usize>,
    /// Calls and events refused: a nested `with` or `run`, an event that found the queue full.
    refused: Cell<u32>,
}

// One task owns the cell at a time (see above).
unsafe impl<T, E: Copy, const N: usize> Sync for Owner<T, E, N> {}

impl<T> Owner<T> {
    /// An owner without events.
    pub const fn new() -> Owner<T> {
        Owner::build(None)
    }
}

impl<T, E: Copy, const N: usize> Owner<T, E, N> {
    /// An owner that takes events: `apply` runs each one on the object (`Reentry::apply`).
    pub const fn with_events(apply: fn(&mut T, E)) -> Owner<T, E, N> {
        Owner::build(Some(apply))
    }

    const fn build(apply: Option<fn(&mut T, E)>) -> Owner<T, E, N> {
        Owner {
            cell: UnsafeCell::new(MaybeUninit::zeroed()),
            live: Cell::new(false),
            busy: Cell::new(false),
            transition: Cell::new(false),
            apply,
            queue: [const { Cell::new(None) }; N],
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
        if self.live.get() || self.transition.replace(true) {
            return Err(E_LOADED);
        }
        // SAFETY: the cell is used by one task at a time; no reference into it is alive here (`with` refuses to nest, and the cell holds no live value).
        let r = init(unsafe { &mut *self.cell.get() });
        self.transition.set(false);
        r?;
        self.live.set(true);
        Ok(self.cell.get() as *mut T)
    }

    /// The instance for the duration of `f`; `None` before `start`, after `stop` and inside another call into the same owner (counted in `refused`).
    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        self.run(None, |t| Some(f(t)))
    }

    /// `f` on the instance as one call: the owner is busy until it returns, then the events that arrived meanwhile run, in order. `refused` is the answer, and `f` is
    /// not called, before `start`, after `stop` and inside another call into the same owner (the latter counted in `refused()`). The tables of a service file reach
    /// the object through this (`Enter`).
    pub fn run<R>(&self, refused: R, f: impl FnOnce(&mut T) -> R) -> R {
        if !self.live.get() || self.transition.get() {
            return refused;
        }
        if self.busy.replace(true) {
            self.refused.set(self.refused.get() + 1);
            return refused;
        }
        // SAFETY: as `start`; `busy` keeps this the only reference, and `live` says the cell holds a value.
        let r = f(unsafe { &mut *(self.cell.get() as *mut T) });
        self.leave();
        r
    }

    /// Hand `event` to the object. When the owner is idle it runs at once; inside a call it is queued, to run right after that call returns (see the type).
    pub fn post(&self, event: E) -> Post {
        if !self.live.get() || self.transition.get() {
            return Post::NoOwner;
        }
        if self.busy.get() {
            let len = self.len.get();
            if len == N {
                self.refused.set(self.refused.get() + 1);
                return Post::Full;
            }
            self.queue[(self.head.get() + len) % N].set(Some(event));
            self.len.set(len + 1);
            return Post::Queued;
        }
        self.busy.set(true);
        if let Some(apply) = self.apply {
            // SAFETY: as in `run`.
            apply(unsafe { &mut *(self.cell.get() as *mut T) }, event);
        }
        self.leave();
        Post::Ran
    }

    /// End of a call: run the queued events in order (each may queue more), then the owner is idle.
    fn leave(&self) {
        while self.len.get() > 0 {
            let at = self.head.get();
            let event = self.queue[at].take();
            self.head.set((at + 1) % N);
            self.len.set(self.len.get() - 1);
            if let (Some(apply), Some(event)) = (self.apply, event) {
                // SAFETY: as in `run`: still busy, so no other reference exists.
                apply(unsafe { &mut *(self.cell.get() as *mut T) }, event);
            }
        }
        self.busy.set(false);
    }

    /// The pointer to the instance, null when there is none (`ProviderHandle` takes it as the instance of the row).
    pub fn instance(&self) -> *mut T {
        if self.live.get() && !self.transition.get() {
            self.cell.get() as *mut T
        } else {
            core::ptr::null_mut()
        }
    }

    /// End the instance: `f` works on it in place (the file's `fini` gives it `Provider::stop`), then it is dropped in place. A hook that fires inside `f` finds none
    /// (`with` and `instance` refuse during the transition) and cannot start or stop. `false` when there was none, and while a `with` closure runs (it holds `&mut T`: the state stays intact).
    pub fn stop(&self, f: impl FnOnce(&mut T)) -> bool {
        if self.busy.get() || !self.live.get() || self.transition.replace(true) {
            return false;
        }
        let at = self.cell.get() as *mut T;
        // SAFETY: `live` was set, so the cell holds a value; `transition` keeps `start`, `stop` and `with` out until it is dropped, once.
        unsafe {
            f(&mut *at);
            core::ptr::drop_in_place(at);
        }
        self.live.set(false);
        self.transition.set(false);
        true
    }
}

impl<T> Default for Owner<T> {
    fn default() -> Self {
        Owner::new()
    }
}

#[cfg(test)]
mod tests;
