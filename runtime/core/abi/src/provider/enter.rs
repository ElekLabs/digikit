use core::ffi::c_void;

/// How a part function of a table reaches the object behind its instance pointer. The thunks of a table form the `&mut P` through this, so an owner can refuse a call that
/// arrives while one of its calls is running (a callback that re-enters the same service, a hook in an interrupt, the other task) instead of letting the thunk form a second
/// `&mut P`. The two required functions are not generic, so a file has one copy of the guard; `enter` is the inlined glue around them.
pub trait Enter<P> {
    /// Start a call on the object behind `inst`: its address, or null when the owner is inside a call already or has no live object (the thunk then answers its refusal).
    ///
    /// # Safety
    /// `inst` is the instance pointer of the table (the one `P` the owner holds).
    unsafe fn begin(inst: *mut c_void) -> *mut P;

    /// End the call `begin` started (after a non-null `begin` only).
    ///
    /// # Safety
    /// As `begin`.
    unsafe fn end(inst: *mut c_void);

    /// Run `f` on the object behind `inst`; `refused` is the answer when the owner is inside a call already (or has no live object): `f` is not called then.
    ///
    /// # Safety
    /// As `begin`.
    #[inline(always)]
    unsafe fn enter<R>(inst: *mut c_void, refused: R, f: impl FnOnce(&mut P) -> R) -> R {
        let at = Self::begin(inst);
        if at.is_null() {
            return refused;
        }
        let r = f(&mut *at);
        Self::end(inst);
        r
    }
}

/// The plain way: a `&mut P` straight from the pointer. Host tests and the static rows of the runtime use it; a service file's `service!` gives its owner's guard instead.
pub struct Plain;

impl<P> Enter<P> for Plain {
    unsafe fn begin(inst: *mut c_void) -> *mut P {
        inst as *mut P
    }
    unsafe fn end(_inst: *mut c_void) {}
}
