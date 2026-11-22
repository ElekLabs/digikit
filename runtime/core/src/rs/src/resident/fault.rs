//! `FaultFns`: the crash capture.
use dkrt_abi::resident::FaultFns;

extern "C" fn panic() -> ! {
    crate::fault::panic()
}

pub(super) static FNS: FaultFns = FaultFns { panic };
