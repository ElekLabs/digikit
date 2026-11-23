//! The commit checks that need only the collection, the keys table and the free rows.
use super::*;
use dkrt_abi::keys::KeysCopy;
use dkrt_abi::mem::iw;
use dkrt_sdk::*;
use dkrt_svc_rows::dynreg::w;

// ---- the commit checks -----------------------------------------------------------------------------------------------------------

/// The numbers of free rows a commit needs.
pub struct Free {
    pub machine_rows: u32,
    pub param_rows: u32,
    pub page_rows: u32,
}

/// The rows the module's own declarations use, as `[uses]` counts them: (machine rows, param rows, page rows) of the lowered records.
pub fn counts_of(b: &Build, p: &Profile) -> (u32, u32, u32) {
    let pages = if p.pages { b.nm as u32 } else { 0 };
    (b.nm as u32, b.nparam as u32, pages)
}

/// The checks of 6.3 that need only the collection, the header's keys table and the free rows. Returns OK or the refusal.
pub fn check_commit(b: &Build, p: &Profile, keys: &KeysCopy, free: &Free) -> i32 {
    if b.err != 0 {
        return b.err;
    }
    // every machine index of a builder call is a row of the keys table
    let mut i = 0;
    while i < b.nm {
        if b.machines[i].index as usize >= keys.nm {
            return E_DECL_ID;
        }
        i += 1;
    }
    // the collected rows do not exceed [uses]
    let (m, pa, pg) = counts_of(b, p);
    if m > keys.machine_rows || pa > keys.param_rows || pg > keys.page_rows {
        return E_DECL_USES;
    }
    // a free row for each
    if m > free.machine_rows {
        return E_TYPE_USED;
    }
    if pa > free.param_rows {
        return E_PARAM_FULL;
    }
    if pg > free.page_rows {
        return E_PAGE_FULL;
    }
    OK
}

/// The free reserved machine, parameter and page rows of the registry.
pub unsafe fn free_rows(inf: *const u32) -> Free {
    let s = crate::records::rstate();
    Free {
        machine_rows: capacity::free_rows(&s.mach_owner, iw(inf, w::M_DYN_N)),
        param_rows: capacity::free_rows(&s.param_owner, iw(inf, w::P_DYN_N)),
        page_rows: capacity::free_rows(&s.page_owner, iw(inf, w::PG_DYN_N)),
    }
}
