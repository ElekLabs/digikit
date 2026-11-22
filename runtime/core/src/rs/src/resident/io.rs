//! `IoFns`: the I/O task's queue.
use dkrt_abi::resident::IoFns;

unsafe extern "C" fn retire(rec: u32) -> bool {
    crate::io::retire(rec as usize)
}

pub(super) static FNS: IoFns = IoFns { retire };
