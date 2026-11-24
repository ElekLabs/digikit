/// The result of a service call: 0 or a negative load code of `dkrt_sdk` (`E_FORMAT`, `E_MISSING`, `E_CONTRACT`, ...). `Result<_, Status>` carries the code.
pub type Status = i32;

/// The C-shaped result of a thunk: 0 or the status of the refusal.
pub(crate) fn code(r: Result<(), Status>) -> i32 {
    match r {
        Ok(()) => dkrt_sdk::OK,
        Err(status) => status,
    }
}
