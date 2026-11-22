//! `FaultFns`: the crash capture. The entry is the owner's own `extern "C"` function.
use dkrt_abi::resident::{FaultFns, Group};

pub(super) static FNS: Group<FaultFns> = Group::new(FaultFns { panic: crate::fault::panic });
