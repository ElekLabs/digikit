//! The registry state of the machines service (`DKRT_REG`: who owns each reserved machine, parameter and page row), its row counts and the texts of a machine row.
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::mem::iw;
use dkrt_abi::shared::Shared;
use dkrt_sdk::*;
use dkrt_svc_rows::dynreg::{w, RegState, MACH_TEXT};

/// Who owns each reserved machine row (module record index + 1, 0 free). It is the one table of the registry that is read outside the owner: the MODULES reply asks
/// `Describe::describe` on the command handler's task, which must answer while the owner is busy in another call (the owner's guard would refuse it). So it is a `Shared`
/// cell of this crate, written by the owner in short sections (a copy of one byte), and read the same way. The rest of the registry state (`RegState`) is a field of the owner.
static MACH_OWNER: Shared<[u8; MAX_MACH]> = Shared::new([0; MAX_MACH]);

/// The owner of machine row `k` (0: free, or `k` past the rows).
pub fn mach_owner<C: CoreApi>(core: &C, k: usize) -> u8 {
    MACH_OWNER.with(core, |o| if k < MAX_MACH { o[k] } else { 0 })
}

/// Machine row `k` belongs to module record `owner` - 1 (0: it is free). The owner's `attach` and `detach` write it; a host test sets it to stand for a module.
pub fn set_mach_owner<C: CoreApi>(core: &C, k: usize, owner: u8) {
    MACH_OWNER.with(core, |o| {
        if k < MAX_MACH {
            o[k] = owner;
        }
    })
}

/// A copy of the owners of all machine rows, one short section.
pub(crate) fn mach_owners<C: CoreApi>(core: &C) -> [u8; MAX_MACH] {
    MACH_OWNER.with(core, |o| *o)
}

/// Every machine row is free (the reset of a host test; the service starts with all rows free).
pub fn clear_mach_owners<C: CoreApi>(core: &C) {
    MACH_OWNER.with(core, |o| *o = [0; MAX_MACH])
}

/// The code of the first run-time parameter row (a row `k` has code `param_first + k`); `u32::MAX` without a registry.
pub fn param_first<C: CoreApi>(core: &C) -> u32 {
    unsafe {
        let inf = dkrt_svc_rows::dynreg(core);
        if inf.is_null() {
            u32::MAX
        } else {
            iw(inf, w::P_DYN0)
        }
    }
}

/// The reserved machine, parameter and page rows as (rows the registry has, rows in use); (0, 0) without a registry (for CAPACITY).
pub(crate) unsafe fn counts<C: CoreApi>(core: &C, s: &RegState) -> [(u32, u32); 3] {
    let inf = dkrt_svc_rows::dynreg(core);
    if inf.is_null() {
        return [(0, 0); 3];
    }
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
        (iw(inf, w::M_DYN_N), used(&mach_owners(core))),
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
    s: &mut RegState,
    k: usize,
    long: *const u8,
    short: *const u8,
    upper: bool,
) -> (u32, u32) {
    copy_text(s.s.mach_name[k].as_mut_ptr(), long, MACH_TEXT);
    copy_text(s.s.mach_short[k].as_mut_ptr(), short, MACH_TEXT);
    (
        dkrt_svc_rows::make_rep(
            s.s.rep_long[k].as_mut_ptr(),
            s.s.mach_name[k].as_ptr(),
            upper,
        ),
        dkrt_svc_rows::make_rep(
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
