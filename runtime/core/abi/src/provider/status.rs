/// The result of a service call: 0 or a negative load code of `dkrt_sdk` (`E_FORMAT`, `E_MISSING`, `E_CONTRACT`, ...). `Result<_, Status>` carries the code.
pub type Status = i32;
