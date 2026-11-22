//! `FaultFns`: the crash capture (`fault.rs`).

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FaultFns {
    /// A panic: write the crash record and stop (the next boot is a safe one).
    pub panic: extern "C" fn() -> !,
}
