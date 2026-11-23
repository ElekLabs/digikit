//! `Owner<T>`: the one cell of a service file that holds its owner object (`Provider`), made by `start` in the file's `init` and taken back by `stop` in `fini`.
use core::cell::{Cell, UnsafeCell};
use core::mem::MaybeUninit;
use dkrt_abi::provider::Status;
use dkrt_sdk::E_LOADED;

/// The cell. A file is one service and has one; `service!` declares it as `DK_SVC_OWNER`, so the file's hooks reach the instance with [`Owner::with`] and the part
/// functions through the pointer `start` returned. Every call runs in the UI task (or in an interrupt that the service guards itself), never concurrently.
pub struct Owner<T> {
    /// The state, uninitialised until `start`: all-zero bytes, so the cell is `.bss` and costs no file bytes.
    cell: UnsafeCell<MaybeUninit<T>>,
    /// Whether `cell` holds a live `T`.
    live: Cell<bool>,
    /// Set while `with` runs a closure: a nested `with` (a hook that fires inside a part function of the same owner) finds nothing, instead of a second `&mut T`.
    busy: Cell<bool>,
    /// Set for the whole of `start`'s `init` and of `stop`'s `f` and drop: `start`, `stop` and `with` refuse meanwhile, and `live` changes only when the transition ends well.
    transition: Cell<bool>,
}

// One task owns the cell at a time (see above).
unsafe impl<T> Sync for Owner<T> {}

impl<T> Owner<T> {
    pub const fn new() -> Owner<T> {
        Owner {
            cell: UnsafeCell::new(MaybeUninit::zeroed()),
            live: Cell::new(false),
            busy: Cell::new(false),
            transition: Cell::new(false),
        }
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

    /// The instance for the duration of `f`; `None` before `start`, after `stop` and inside another `with` of the same cell.
    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        if !self.live.get() || self.transition.get() || self.busy.replace(true) {
            return None;
        }
        // SAFETY: as `start`; `busy` keeps this the only reference, and `live` says the cell holds a value.
        let r = f(unsafe { &mut *(self.cell.get() as *mut T) });
        self.busy.set(false);
        Some(r)
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
