//! The walk over a validated `register` section. The words of `dynreg_info` and the machines registry's state are the services' (`dkrt_svc_params::dynreg`).
use crate::mem::rd32;

/// Walk the records of a validated section. Calls `f(kind, body)` for each.
pub unsafe fn each(reg: *const u8, mut f: impl FnMut(u32, *const u8)) {
    let n = rd32(reg.add(12));
    let mut at = 16usize;
    let mut i = 0;
    while i < n {
        let kind = rd32(reg.add(at));
        let len = rd32(reg.add(at + 4)) as usize;
        f(kind, reg.add(at + 8));
        at += len;
        i += 1;
    }
}
