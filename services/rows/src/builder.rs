//! The plumbing the builder calls of the machines and fx services share (docs/plans/sdk.md 6.2): handles that are addresses of elements of the service's own state, the
//! placeholder a refused call returns, text copied into fixed fields, the first error kept, the bound on records, and the gate that says which module's `init` is open.
//! Each service owns its state, its placeholder and its gate (statics of the service); this crate owns the rules.
use core::ptr::{addr_of, read_volatile, write_volatile};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::mem::cstr_len;
use dkrt_sdk::*;

/// The index of the element `h` points to in `a`, None for the placeholder or any other pointer.
pub fn find<T>(a: &[T], h: *mut u8) -> Option<usize> {
    let base = a.as_ptr() as usize;
    let at = h as usize;
    let size = core::mem::size_of::<T>();
    if at < base || at >= base + a.len() * size || (at - base) % size != 0 {
        return None;
    }
    Some((at - base) / size)
}

/// Copy the C string `src` into the NUL-padded field `dst`: null, empty, or without room for the NUL is `E_FORMAT` in `err`.
///
/// # Safety
/// `src` is null or a NUL-terminated string (at most `dst.len()` bytes are read).
pub unsafe fn put_text(dst: &mut [u8], src: *const u8, err: &mut i32) {
    if src.is_null() {
        *err = E_FORMAT;
        return;
    }
    let n = cstr_len(src, dst.len());
    if n == 0 || n == dst.len() {
        *err = E_FORMAT; // empty, or no room for the NUL
        return;
    }
    let mut i = 0;
    while i < n {
        dst[i] = *src.add(i);
        i += 1;
    }
}

/// The length of the NUL-terminated text in a field.
pub fn len_of(s: &[u8]) -> usize {
    let mut n = 0;
    while n < s.len() && s[n] != 0 {
        n += 1;
    }
    n
}

/// Keep the first error.
pub fn fail(err: &mut i32, e: i32) {
    if *err == 0 {
        *err = e;
    }
}

/// One more record of a collection bounded by `max`; false (`E_DECL_FULL` kept) when it is full.
pub fn room(err: &mut i32, records: &mut usize, max: usize) -> bool {
    if *records >= max {
        fail(err, E_DECL_FULL);
        return false;
    }
    *records += 1;
    true
}

/// Which module's `init` the calls of a service belong to, and the placeholder the service hands out for a refused call. A service has one, as a static: `begin` at
/// `module_begin`, `end` when its commit is over or the module is removed.
pub struct Gate {
    token: u32,
    open: u32,
}

impl Gate {
    pub const fn new() -> Gate {
        Gate {
            token: 0,
            open: u32::MAX,
        }
    }
    /// The placeholder handle: the address of this gate, no element of any state.
    pub fn placeholder(&self) -> *mut u8 {
        addr_of!(self.token) as usize as *mut u8
    }
    /// A handle that is not null, not the placeholder and not an element of the service's state: it came from another service (or from nowhere).
    pub fn foreign(&self, h: *mut u8) -> bool {
        !h.is_null() && h != self.placeholder()
    }
    /// `module_begin`: the `init` of module record `rec` is open.
    pub fn begin(&mut self, rec: usize) {
        unsafe { write_volatile(&mut self.open, rec as u32) }
    }
    /// Close the gate: later builder calls get the placeholder.
    pub fn end(&mut self) {
        unsafe { write_volatile(&mut self.open, u32::MAX) }
    }
    /// The record whose `init` is open (None: none).
    pub fn rec(&self) -> Option<usize> {
        match unsafe { read_volatile(&self.open) } {
            u32::MAX => None,
            r => Some(r as usize),
        }
    }
    /// Is `init` of module record `rec` open?
    pub fn is_open_for(&self, rec: usize) -> bool {
        self.rec() == Some(rec)
    }
    /// A call that names a handle: the gate is open and the code that runs is the module whose `init` it is (`CoreApi::current_source`, record + 1). A call from
    /// another module's code (or from a callback after `init`) is refused.
    pub fn open_any<C: CoreApi>(&self, core: &C) -> bool {
        match self.rec() {
            Some(rec) => core.current_source() as usize == rec + 1,
            None => false,
        }
    }
    /// A call that opens a handle with the module context `ctx`: the gate is open and `ctx` is the context of the module whose `init` it is (`CoreApi::module_of`).
    ///
    /// # Safety
    /// `ctx` is any pointer: the core only compares it.
    pub unsafe fn open_for<C: CoreApi>(&self, core: &C, ctx: *mut u8) -> bool {
        match self.rec() {
            Some(rec) => core.module_of(ctx) as usize == rec + 1,
            None => false,
        }
    }
}
