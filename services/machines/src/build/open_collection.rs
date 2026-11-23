//! The open collection of the module in `init`: the collection, the gate and the placeholder handle.
use super::*;
use core::ptr::addr_of_mut;
use dkrt_svc_rows::builder::Gate;

/// Zero at the start (a `.bss` of the service file, not 5 KiB of initialised data in it): `begin` writes `Build::new()` before the first builder call of every module.
pub(super) static mut BUILD: Build = unsafe { core::mem::zeroed() };
pub(super) static mut GATE: Gate = Gate::new();

pub(super) fn gate() -> &'static mut Gate {
    unsafe { &mut *addr_of_mut!(GATE) }
}

/// The placeholder handle of a refused call.
pub fn placeholder() -> *mut u8 {
    gate().placeholder()
}

/// The collection (host tests read it).
pub fn build() -> &'static mut Build {
    unsafe { &mut *addr_of_mut!(BUILD) }
}

/// `module_begin`: forget what an earlier module collected and open the collection for module record `rec`.
pub unsafe fn begin(rec: usize) {
    core::ptr::write(addr_of_mut!(BUILD), Build::new());
    *addr_of_mut!(ARGS) = None;
    gate().begin(rec);
}

/// Close the collection: later builder calls return the placeholder.
pub unsafe fn end() {
    gate().end();
}
