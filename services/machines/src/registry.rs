//! The registry state of the machines service (`DKRT_REG`: who owns each reserved machine, parameter and page row), its row counts and the texts of a machine row.
use core::ptr::{addr_of, addr_of_mut};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::mem::iw;
use dkrt_sdk::*;
use dkrt_svc_params::dynreg::{w, RegState, MACH_TEXT};

/// The registry's state: who owns each reserved machine, parameter and page row (module record index + 1, 0 free) and the strings the rows use. One machines
/// service, one state: a static of this crate, so it lives in `machines.dks` (the symbol stays for the test tools).
#[no_mangle]
pub static mut DKRT_REG: RegState = unsafe { core::mem::zeroed() };

pub fn rstate() -> &'static mut RegState {
    unsafe { &mut *addr_of_mut!(DKRT_REG) }
}

/// The code of the first run-time parameter row (a row `k` has code `param_first + k`); `u32::MAX` without a registry.
pub fn param_first<C: CoreApi>(core: &C) -> u32 {
    unsafe {
        let inf = dkrt_svc_params::dynreg(core);
        if inf.is_null() {
            u32::MAX
        } else {
            iw(inf, w::P_DYN0)
        }
    }
}

/// The owner of each run-time parameter row.
pub fn param_owners() -> &'static [u8] {
    unsafe { &(*addr_of!(DKRT_REG)).param_owner }
}

/// The reserved machine, parameter and page rows as (rows the registry has, rows in use); (0, 0) without a registry (for CAPACITY).
pub(crate) unsafe fn counts<C: CoreApi>(core: &C) -> [(u32, u32); 3] {
    let inf = dkrt_svc_params::dynreg(core);
    if inf.is_null() {
        return [(0, 0); 3];
    }
    let s = rstate();
    let used = |o: &[u8]| {
        let mut n = 0;
        let mut k = 0;
        while k < o.len() {
            if o[k] != 0 {
                n += 1;
            }
            k += 1;
        }
        n
    };
    [
        (iw(inf, w::M_DYN_N), used(&s.mach_owner)),
        (iw(inf, w::P_DYN_N), used(&s.param_owner)),
        (iw(inf, w::PG_DYN_N), used(&s.page_owner)),
    ]
}

pub(crate) const KNOB_PARAM: u32 = 0x8000_0000;
pub(crate) const MAX_MACH: usize = limits::MACHINE_ROWS;
pub(crate) const MAX_PARAM: usize = limits::PARAM_ROWS;
pub(crate) const MAX_PAGE: usize = limits::PAGE_ROWS;
pub(crate) const KNOBS: usize = limits::PAGE_KNOBS;

/// The texts of machine row `k`: the long and short name copied into the row's own buffers (`MACH_TEXT` bytes each) and their firmware string representations
/// (upper-cased when `upper`). Returns the representations. One copy of the code for a module's rows and for the MISSING placeholder.
#[inline(never)]
pub(crate) unsafe fn machine_text(
    k: usize,
    long: *const u8,
    short: *const u8,
    upper: bool,
) -> (u32, u32) {
    let s = rstate();
    copy_text(s.s.mach_name[k].as_mut_ptr(), long, MACH_TEXT);
    copy_text(s.s.mach_short[k].as_mut_ptr(), short, MACH_TEXT);
    (
        dkrt_svc_params::make_rep(
            s.s.rep_long[k].as_mut_ptr(),
            s.s.mach_name[k].as_ptr(),
            upper,
        ),
        dkrt_svc_params::make_rep(
            s.s.rep_short[k].as_mut_ptr(),
            s.s.mach_short[k].as_ptr(),
            upper,
        ),
    )
}

pub(crate) unsafe fn copy_text(dst: *mut u8, src: *const u8, n: usize) {
    let mut i = 0;
    while i < n {
        *dst.add(i) = *src.add(i);
        i += 1;
    }
}
