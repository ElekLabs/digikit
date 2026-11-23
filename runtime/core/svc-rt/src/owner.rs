//! `Owner<T>`: the one cell of a service file that holds its owner object (`Provider`), made by `start` in the file's `init` and taken back by `stop` in `fini`.
use core::cell::{Cell, UnsafeCell};
use dkrt_abi::provider::Status;
use dkrt_sdk::E_LOADED;

/// The cell. A file is one service and has one; `service!` declares it as `DK_SVC_OWNER`, so the file's hooks reach the instance with [`Owner::with`] and the part
/// functions through the pointer `start` returned. Every call runs in the UI task (or in an interrupt that the service guards itself), never concurrently.
pub struct Owner<T> {
    cell: UnsafeCell<Option<T>>,
    /// Set while `with` runs a closure: a nested `with` (a hook that fires inside a part function of the same owner) finds nothing, instead of a second `&mut T`.
    busy: Cell<bool>,
}

// One task owns the cell at a time (see above).
unsafe impl<T> Sync for Owner<T> {}

impl<T> Owner<T> {
    pub const fn new() -> Owner<T> {
        Owner {
            cell: UnsafeCell::new(None),
            busy: Cell::new(false),
        }
    }

    /// Make the instance with `make` and keep it; the pointer to it is what the row's part functions work on. `E_LOADED` when the cell holds one already: `make` is not called.
    pub fn start(&self, make: impl FnOnce() -> Result<T, Status>) -> Result<*mut T, Status> {
        // SAFETY: the cell is used by one task at a time; no reference into it is alive here (`with` refuses to nest and a refused start made none).
        let slot = unsafe { &mut *self.cell.get() };
        if slot.is_some() {
            return Err(E_LOADED);
        }
        *slot = Some(make()?);
        Ok(slot.as_mut().map_or(core::ptr::null_mut(), |t| t as *mut T))
    }

    /// The instance for the duration of `f`; `None` before `start`, after `stop` and inside another `with` of the same cell.
    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        if self.busy.replace(true) {
            return None;
        }
        // SAFETY: as `start`; `busy` keeps this the only reference.
        let r = unsafe { (*self.cell.get()).as_mut() }.map(f);
        self.busy.set(false);
        r
    }

    /// The pointer to the instance, null when there is none (`ProviderHandle` takes it as the instance of the row).
    pub fn instance(&self) -> *mut T {
        // SAFETY: as `start`; only the address is taken.
        match unsafe { (*self.cell.get()).as_mut() } {
            Some(t) => t as *mut T,
            None => core::ptr::null_mut(),
        }
    }

    /// Take the instance out: the file's `fini` gives it `Provider::stop`. `None` when there is none.
    pub fn stop(&self) -> Option<T> {
        // SAFETY: as `start`.
        unsafe { (*self.cell.get()).take() }
    }
}

impl<T> Default for Owner<T> {
    fn default() -> Self {
        Owner::new()
    }
}

#[cfg(test)]
mod tests;
