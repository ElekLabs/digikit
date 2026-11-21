//! The guard of a firmware call. The address of a firmware function is bound when the service file is bound (the port file); a firmware version that has no such function leaves
//! it 0, and a call to 0 would jump to the start of memory. The call is then a no-op with the neutral answer, as the table-driven wrapper was for a zero word.

/// `call()` when `addr` (the bound address of the firmware function) is not 0, else `neutral`. `black_box`: the address of a linked function is never 0 to the compiler, and
/// the check must stay.
pub fn call_if_bound<R>(addr: usize, neutral: R, call: impl FnOnce() -> R) -> R {
    if core::hint::black_box(addr) == 0 {
        neutral
    } else {
        call()
    }
}
