//! The run-time registry: machines and parameters that a loaded `.dkm` adds to the rows `core/dynreg` reserved at patch
//! time (docs/design/digikit-registry.md). Everything here runs in the UI task. The loader calls it (`apply` from the completion step of the boot-time load, `remove` from the undo); it never calls the loader.
//!
//! The runtime does not link to `core/machines` or `core/params`. `core/dynreg` exports one table, `dynreg_info`, with the
//! addresses of the rows and tables and the row layouts; the link is optional, so an image with the runtime and without
//! `core/dynreg` loads modules that carry no `register` records and refuses the ones that do (`E_NO_REGISTRY`).
//!
//! Digitone II keeps a machine's knobs in page rows (`pages`), not in the machine row: a PAGE record fills one reserved page
//! row; the machine record's `own_dsp` is written to the row (the frame builder then tells the DSP the machine's own type).
//!
//! Rules: all checks run before the first write (`check`); rows are written in an order that a reader never sees a
//! half-filled machine (the name pointer, which marks a row as used, is the last store, one aligned 32-bit word); the
//! strings the rows point to live in this module's own state, never in the arena, so unload cannot leave a dangling pointer.
use crate::handlers::{ApplyCtx, CheckCtx, ServiceList, Services};
use crate::rd32;
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use dkrt_sdk::*;

const REG_MAGIC: u32 = 0x444b_5247; // "DKRG"
const INFO_MAGIC: u32 = 0x4459_4e52; // "DYNR"
const MAX_RECORDS: u32 = dkneeds::MAX_RECORDS;
const MAX_MACH: usize = limits::MACHINE_ROWS;
const MAX_PARAM: usize = limits::PARAM_ROWS;
const MAX_PAGE: usize = limits::PAGE_ROWS;
const KNOBS: usize = limits::PAGE_KNOBS;
pub(crate) const MACH_TEXT: usize = 24;
const REP_SIZE: usize = 48;

// The parameter-row writers live in `params.rs` (core); these paths keep their callers.
pub(crate) use crate::params::{make_rep, reset_param_row, write_fx_param_row, write_param_row, write_send_param_row};

extern "C" {
    /// Data symbol of core/dynreg; resolves to 0 when the module is not in the image (`[object] optional`).
    fn dynreg_info();
}

/// Words of `dynreg_info` (byte offsets / 4). Words 48, 49 and 61 to 64 were the machine-view hook and the in-use functions of hot
/// reload (archived, docs/plans/digikit-hot-reload.md); the table keeps them as zeros so the other offsets stay.
pub(crate) mod w {
    pub const MAGIC: usize = 0;
    pub const DEVICE: usize = 2;
    pub const M_ROWS: usize = 3;
    pub const M_ROW_SIZE: usize = 4;
    pub const M_FIRST: usize = 5;
    pub const M_DYN0: usize = 7;
    pub const M_DYN_N: usize = 8;
    pub const NAMES: usize = 9;
    pub const PERMIT: usize = 10;
    pub const PERTYPE: usize = 11;
    pub const O_LONG: usize = 12;
    pub const O_SHORT: usize = 13;
    pub const O_F0: usize = 14;
    pub const O_CLONE: usize = 15;
    pub const O_POS: usize = 16;
    pub const O_OWN: usize = 17;
    pub const O_NAME: usize = 18;
    pub const O_SNAME: usize = 19;
    pub const P_TABLE: usize = 20;
    pub const P_ROWS: usize = 21;
    pub const P_ROW_SIZE: usize = 22;
    pub const P_FIRST: usize = 23;
    pub const P_DYN0: usize = 25;
    pub const P_DYN_N: usize = 26;
    pub const PF_SIZE: usize = 27;
    pub const P_TEMPLATE_OFF: usize = 28;
    pub const EMPTY: usize = 29;
    pub const SPEC: usize = 30; // 15 words
    pub const KNOBS: usize = 45;
    pub const SLOT_BASE: usize = 46;
    pub const RESERVE_TEMPLATE: usize = 47;
    pub const PG_ROWS: usize = 50; // 0: the device keeps its knobs in the machine row
    pub const PG_ROW_SIZE: usize = 51;
    pub const PG_FIRST: usize = 52;
    pub const PG_DYN0: usize = 53;
    pub const PG_DYN_N: usize = 54;
    pub const PGO_NAME: usize = 55;
    pub const PGO_TITLE: usize = 56;
    pub const PGO_C: usize = 57;
    pub const PGO_MACH: usize = 58;
    pub const PGO_S: usize = 59;
    pub const PG_FREE: usize = 60;
    pub const VERSION: usize = 1;
    // the builder profile (version 3): words 65 to 86
    pub const P_CLONE: usize = 65;
    pub const P_LEVEL: usize = 66;
    pub const P_OWN: usize = 67;
    pub const P_TUNE: usize = 68;
    pub const P_TUNE_SLOT: usize = 69;
    pub const P_LEVEL_CODE: usize = 70;
    pub const P_LEVEL_KNOB: usize = 71;
    pub const P_NSLOTS: usize = 72;
    pub const P_SLOTS: usize = 73; // 8 words
    pub const P_TPL: usize = 81; // 6 words
}

#[repr(C, align(4))]
pub(crate) struct Strings {
    pub(crate) mach_name: [[u8; MACH_TEXT]; MAX_MACH],
    pub(crate) mach_short: [[u8; MACH_TEXT]; MAX_MACH],
    pub(crate) rep_long: [[u8; REP_SIZE]; MAX_MACH],
    pub(crate) rep_short: [[u8; REP_SIZE]; MAX_MACH],
    pub(crate) param_name: [[u8; 32]; MAX_PARAM],
    pub(crate) param_short: [[u8; 16]; MAX_PARAM],
    pub(crate) page_name: [[u8; MACH_TEXT]; MAX_PAGE],
    pub(crate) page_title: [[u8; MACH_TEXT]; MAX_PAGE],
    pub(crate) rep_pname: [[u8; REP_SIZE]; MAX_PAGE],
    pub(crate) rep_ptitle: [[u8; REP_SIZE]; MAX_PAGE],
}

/// The registry's state: who owns each reserved row (module record index + 1, 0 = free) and the strings the rows use.
#[repr(C, align(4))]
pub struct RegState {
    pub magic: u32,
    pub registers: u32,
    pub removes: u32,
    pub mach_owner: [u8; MAX_MACH],
    pub param_owner: [u8; MAX_PARAM],
    pub page_owner: [u8; MAX_PAGE],
    /// An empty std::string representation (length 0, capacity 0, reference count -1, NUL) for a free machine row.
    pub(crate) empty_rep: [u32; 4],
    pub(crate) s: Strings,
}

#[no_mangle]
pub static mut DKRT_REG: RegState = unsafe { core::mem::zeroed() };

pub(crate) fn rs() -> &'static mut RegState {
    unsafe { &mut *addr_of_mut!(DKRT_REG) }
}

pub(crate) unsafe fn info() -> *const u32 {
    #[cfg(test)]
    {
        return crate::handlers::tests::dynreg_table();
    }
    #[cfg(not(test))]
    info_table()
}

#[cfg(not(test))]
#[inline(always)]
unsafe fn info_table() -> *const u32 {
    let a = core::hint::black_box(dynreg_info as usize);
    if a == 0 {
        return core::ptr::null();
    }
    let p = a as *const u32;
    if read_volatile(p.add(w::MAGIC)) != INFO_MAGIC {
        return core::ptr::null();
    }
    p
}

/// The device facts of the builder lowering (`collect::lower`) from `dynreg_info` version 3; None without a registry or with an older table.
pub unsafe fn profile() -> Option<crate::collect::Profile> {
    let inf = info();
    if inf.is_null() || iw(inf, w::VERSION) < 3 {
        return None;
    }
    let mut p = crate::collect::Profile {
        device: iw(inf, w::DEVICE),
        pages: iw(inf, w::PG_ROWS) != 0,
        slot_base: iw(inf, w::SLOT_BASE),
        clone_of: iw(inf, w::P_CLONE),
        level: iw(inf, w::P_LEVEL),
        own_dsp: iw(inf, w::P_OWN) != 0,
        tune_code: iw(inf, w::P_TUNE),
        tune_slot: iw(inf, w::P_TUNE_SLOT),
        level_code: iw(inf, w::P_LEVEL_CODE),
        level_knob: iw(inf, w::P_LEVEL_KNOB),
        nslots: iw(inf, w::P_NSLOTS).min(8),
        slots: [0; 8],
        tpl: [0; 6],
    };
    let mut i = 0;
    while i < 8 {
        p.slots[i] = iw(inf, w::P_SLOTS + i);
        i += 1;
    }
    i = 0;
    while i < 6 {
        p.tpl[i] = iw(inf, w::P_TPL + i);
        i += 1;
    }
    Some(p)
}

/// Free reserved machine, parameter and page rows, for the commit of a builder module.
pub unsafe fn free_rows() -> Option<crate::collect::Free> {
    let inf = info();
    if inf.is_null() {
        return None;
    }
    let s = rs();
    Some(crate::collect::Free {
        machine_rows: capacity::free_rows(&s.mach_owner, iw(inf, w::M_DYN_N)),
        param_rows: capacity::free_rows(&s.param_owner, iw(inf, w::P_DYN_N)),
        page_rows: capacity::free_rows(&s.page_owner, iw(inf, w::PG_DYN_N)),
    })
}

/// The arguments of the callbacks of the records `apply` is about to write (modules): the k-th ACTION record's `arg` and the k-th
/// native page record's `arg`. Set by the commit just before `apply`, cleared by it.
pub struct ApplyArgs {
    pub click: [u32; 32],
    pub native: [u32; 8],
}
static mut ARGS: Option<ApplyArgs> = None;

pub unsafe fn set_args(a: Option<ApplyArgs>) {
    *addr_of_mut!(ARGS) = a;
}

/// The code of the first run-time parameter row (a row `k` of the registry has code `param_first() + k`); `u32::MAX` without a registry.
pub fn param_first() -> u32 {
    unsafe {
        let inf = info();
        if inf.is_null() {
            u32::MAX
        } else {
            iw(inf, w::P_DYN0)
        }
    }
}

pub(crate) unsafe fn iw(i: *const u32, k: usize) -> u32 {
    read_volatile(i.add(k))
}

pub(crate) unsafe fn rd(a: u32) -> u32 {
    read_volatile(a as *const u32)
}

pub(crate) unsafe fn wr(a: u32, v: u32) {
    write_volatile(a as *mut u32, v)
}

/// Walk the records of a validated section. Calls `f(kind, body)` for each.
pub(crate) unsafe fn each(reg: *const u8, mut f: impl FnMut(u32, *const u8)) {
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

/// Validate the `register` records of a module file. `reg` points into the file (the staging buffer). Nothing is written.
pub unsafe fn check(reg: *const u8, size: u32, image_size: u32) -> i32 {
    check_with::<Services>(reg, size, image_size)
}

/// The walker of `check`: the layout of the section against the lengths the services declare, then each service's own records.
pub(crate) unsafe fn check_with<L: ServiceList>(reg: *const u8, size: u32, image_size: u32) -> i32 {
    if size == 0 {
        return OK;
    }
    let inf = info();
    if inf.is_null() || !L::fits(inf) {
        return E_NO_REGISTRY;
    }
    if size < 16 || rd32(reg) != REG_MAGIC || rd32(reg.add(4)) != 1 {
        return E_FORMAT;
    }
    if rd32(reg.add(8)) != iw(inf, w::DEVICE) {
        return E_REG_DEVICE;
    }
    let n = rd32(reg.add(12));
    if n > MAX_RECORDS {
        return E_FORMAT;
    }
    // the layout: every record has a kind some service declared, and that kind's length
    let mut at: u32 = 16;
    let mut i = 0;
    while i < n {
        if at > size || size - at < 8 {
            return E_FORMAT;
        }
        let kind = rd32(reg.add(at as usize));
        let len = rd32(reg.add(at as usize + 4));
        let want = match L::len(kind) {
            Some(want) => want,
            None => return E_FORMAT,
        };
        if len != want || size - at < len {
            return E_FORMAT;
        }
        at += len;
        i += 1;
    }
    if at != size {
        return E_FORMAT;
    }
    L::check(&CheckCtx { reg, image_size, inf })
}

/// Fill the reserved rows from the (checked) records. `owner` = module record index + 1.
pub unsafe fn apply(owner: u8, reg: *const u8, size: u32, base: u32) -> i32 {
    apply_with::<Services>(owner, reg, size, base)
}

/// The walker of `apply`: each service writes its records, in list order; the first failure stops the walk.
pub(crate) unsafe fn apply_with<L: ServiceList>(owner: u8, reg: *const u8, size: u32, base: u32) -> i32 {
    if size == 0 {
        return OK;
    }
    let inf = info();
    if inf.is_null() {
        return E_NO_REGISTRY;
    }
    let rc = L::apply(&ApplyCtx { owner, reg, base, inf, args: (*addr_of!(ARGS)).as_ref() });
    *addr_of_mut!(ARGS) = None;
    if rc != OK {
        return rc;
    }
    let s = rs();
    s.registers = s.registers.wrapping_add(1);
    OK
}

/// The reserved machine, parameter and page rows as (rows the registry has, rows in use); (0, 0) without a registry (for CAPACITY).
pub unsafe fn counts() -> [(u32, u32); 3] {
    let inf = info();
    if inf.is_null() {
        return [(0, 0); 3];
    }
    let s = rs();
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

/// The machine type numbers of the machine rows `owner` filled (at most 8), in type order; returns how many.
pub fn types_of(owner: u8, out: &mut [u8; MAX_MACH]) -> usize {
    let s = rs();
    let first = dkrt_sdk::device::DYN_TYPE_FIRST;
    let mut n = 0;
    let mut k = 0;
    while k < MAX_MACH {
        if s.mach_owner[k] == owner && n < MAX_MACH {
            out[n] = (first + k as u32) as u8;
            n += 1;
        }
        k += 1;
    }
    n
}


/// Give back every row `owner` filled: each service gives back its own, the last service of the list first.
pub unsafe fn remove(owner: u8) {
    remove_with::<Services>(owner)
}

pub(crate) unsafe fn remove_with<L: ServiceList>(owner: u8) {
    L::remove(owner);
    if !info().is_null() {
        let s = rs();
        s.removes = s.removes.wrapping_add(1);
    }
}
