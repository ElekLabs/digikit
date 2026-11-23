use core::ffi::c_void;

/// How a part function of a table reaches the object behind its instance pointer. The thunks of a table form the `&mut P` through this, so an owner can refuse a call that
/// arrives while one of its calls is running (a callback that re-enters the same service) instead of letting the thunk form a second `&mut P`.
pub trait Enter<P> {
    /// Run `f` on the object behind `inst`; `refused` is the answer when the owner is inside a call already (or has no live object): `f` is not called then.
    ///
    /// # Safety
    /// `inst` is the instance pointer of the table (the one `P` the owner holds).
    unsafe fn enter<R>(inst: *mut c_void, refused: R, f: impl FnOnce(&mut P) -> R) -> R;
}

/// The plain way: a `&mut P` straight from the pointer. Host tests and the static rows of the runtime use it; a service file's `service!` gives its owner's guard instead.
pub struct Plain;

impl<P> Enter<P> for Plain {
    unsafe fn enter<R>(inst: *mut c_void, _refused: R, f: impl FnOnce(&mut P) -> R) -> R {
        f(&mut *(inst as *mut P))
    }
}
