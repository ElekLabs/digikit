//! `machines` service, record handlers (interface B): the machine, parameter, page, click-action and native-page records of the
//! `register` section, and the voice parts of the DSP section. The registry walker (`registry.rs`) calls them through
//! `handlers::Service`; the rows and strings they fill are the registry's state (`DKRT_REG`, a static of this service).
use crate::{dsp, nativepage, slots};
use core::ptr::{addr_of, addr_of_mut};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::DspSec;
use dkrt_abi::mem::{cstr_len, iw, rd, rd32, wr};
use dkrt_abi::registry::each;
use dkrt_abi::service::{
    check_step, tick_phase, ApplyCtx, CheckCtx, Claim, Declare, GuardState, ModuleCommitCtx,
    RowWrite, Service, SettleCtx,
};
use dkrt_sdk::records::{
    KIND_ACTION, KIND_MACHINE, KIND_NATIVE_PAGE, KIND_NATIVE_PAGE_V2, KIND_PAGE, KIND_PARAM,
    LEN_ACTION, LEN_MACHINE, LEN_NATIVE_PAGE, LEN_NATIVE_PAGE_V2, LEN_PAGE, LEN_PARAM,
};
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
unsafe fn counts<C: CoreApi>(core: &C) -> [(u32, u32); 3] {
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

const KNOB_PARAM: u32 = 0x8000_0000;
const MAX_MACH: usize = limits::MACHINE_ROWS;
const MAX_PARAM: usize = limits::PARAM_ROWS;
const MAX_PAGE: usize = limits::PAGE_ROWS;
const KNOBS: usize = limits::PAGE_KNOBS;

/// The machines service.
pub struct Machines;

impl<C: CoreApi> Service<C> for Machines {
    unsafe fn owned_ids(_core: &C, owner: u8, out: &mut [u8; 8]) -> u32 {
        types_of(owner, out) as u32
    }

    unsafe fn declare(d: &mut Declare) {
        d.kind(KIND_PARAM, LEN_PARAM);
        d.kind(KIND_MACHINE, LEN_MACHINE);
        d.kind(KIND_PAGE, LEN_PAGE);
        d.kind(KIND_ACTION, LEN_ACTION);
        d.kind(KIND_NATIVE_PAGE, LEN_NATIVE_PAGE);
        d.kind(KIND_NATIVE_PAGE_V2, LEN_NATIVE_PAGE_V2);
    }

    unsafe fn settle(core: &C, s: &SettleCtx) {
        crate::assign::plan(core, s)
    }

    unsafe fn check(core: &C, step: u32, c: &CheckCtx) -> i32 {
        use check_step as st;
        let inf = dkrt_svc_params::dynreg(core);
        if inf.is_null() || !fits(inf) {
            // the machine, parameter, page, action and native page records need this registry: a section without them does not
            let none = counts_of(c.reg) == (0, 0, 0, 0, 0);
            return if step == st::COUNTS && !none {
                E_NO_REGISTRY
            } else {
                OK
            };
        }
        let c = &Ctx {
            reg: c.reg,
            image_size: c.image_size,
            inf,
        };
        if step == st::COUNTS {
            ck_counts(c)
        } else if step == st::ACTIONS {
            ck_actions(c)
        } else if step == st::NATIVE {
            ck_native(core, c)
        } else if step == st::DEVICE {
            ck_device(c)
        } else if step == st::RECORDS {
            ck_records(core, c)
        } else if step == st::OWNERS {
            ck_owners(core, c)
        } else if step == st::ROWS {
            ck_rows(core, c)
        } else {
            OK
        }
    }

    unsafe fn apply(core: &C, c: &ApplyCtx) -> i32 {
        let inf = dkrt_svc_params::dynreg(core);
        if inf.is_null() {
            return if counts_of(c.reg) == (0, 0, 0, 0, 0) {
                OK
            } else {
                E_NO_REGISTRY
            };
        }
        apply(
            core,
            &AppCtx {
                owner: c.owner,
                reg: c.reg,
                base: c.base,
                inf,
            },
        )
    }

    unsafe fn apply_record(core: &C, kind: u32, body: *const u8, c: &ApplyCtx) -> i32 {
        apply_record(core, kind, body, c)
    }

    unsafe fn remove(core: &C, owner: u8) {
        crate::build::end();
        remove(core, owner)
    }

    unsafe fn module_begin(_core: &C, rec: usize) {
        crate::build::begin(rec)
    }

    unsafe fn module_commit(core: &C, rec: usize, c: &ModuleCommitCtx) -> i32 {
        crate::build::commit(core, rec, c)
    }

    unsafe fn tick(core: &C, phase: u32) {
        if phase == tick_phase::LAST {
            slots::tick(core);
        }
    }

    unsafe fn cap(core: &C, kind: u8) -> Option<(u32, u32)> {
        use dkrt_sdk::proto::*;
        // an if chain, not a `match`: a dense match becomes a PC-relative jump table, which is not ColdFire code
        if kind == CAPK_MACHINE_ROWS {
            Some(counts(core)[0])
        } else if kind == CAPK_VOICE_ROWS {
            Some(dsp::rows_counts(core))
        } else if kind == CAPK_PARAM_ROWS {
            Some(counts(core)[1])
        } else if kind == CAPK_PAGE_ROWS {
            Some(counts(core)[2])
        } else if kind == CAPK_MAP_ENTRIES {
            Some((dkrt_sdk::sel::MAP_N as u32, crate::assign::map_used()))
        } else {
            None
        }
    }

    unsafe fn attribute(core: &C, site: u32, id: u32) -> Option<usize> {
        if site == dkrt_sdk::fault::SITE_VOICE {
            dsp::module_of_type(core, id)
        } else {
            None
        }
    }

    unsafe fn guard_state(core: &C, ty: u32) -> Option<GuardState> {
        Some(GuardState {
            faults: core.guard_faults(ty),
            disabled: core.guard_disabled(ty),
        })
    }

    unsafe fn dsp_check_sections(core: &C, sec: &DspSec) -> i32 {
        dsp::check_sections(core, sec)
    }

    unsafe fn dsp_check_rows(core: &C, sec: &DspSec) -> i32 {
        dsp::check_rows(core, sec)
    }

    unsafe fn dsp_check_types(core: &C, sec: &DspSec) -> i32 {
        dsp::check_types(core, sec)
    }

    unsafe fn dsp_claim(core: &C, c: &mut Claim) {
        dsp::claim(core, c)
    }

    unsafe fn dsp_release(core: &C, rec: usize) {
        dsp::release(core, rec)
    }

    unsafe fn dsp_clear(core: &C, rec: usize) -> bool {
        dsp::clear_rows(core, rec)
    }

    unsafe fn dsp_row_step(core: &C, rec: usize, want: &mut u32, write: bool) -> Option<RowWrite> {
        dsp::row_step(core, rec, want, write)
    }
}

/// What the checks read: the section and the words of `dynreg_info`.
struct Ctx {
    reg: *const u8,
    image_size: u32,
    inf: *const u32,
}

/// What `apply` reads: the owner, the section, the load address and the words of `dynreg_info`.
struct AppCtx {
    owner: u8,
    reg: *const u8,
    base: u32,
    inf: *const u32,
}

/// The table must fit this registry's state (a table that says more rows than the state has would index past it).
unsafe fn fits(inf: *const u32) -> bool {
    !(iw(inf, w::M_DYN_N) as usize > MAX_MACH
        || iw(inf, w::P_DYN_N) as usize > MAX_PARAM
        || iw(inf, w::PG_DYN_N) as usize > MAX_PAGE)
}

/// A NUL-terminated printable-ASCII text of exactly `field` bytes, 1 to `field - 1` characters.
unsafe fn text_ok(p: *const u8, field: usize) -> bool {
    let n = cstr_len(p, field);
    if n == 0 || n == field {
        return false;
    }
    let mut i = 0;
    while i < field {
        let c = *p.add(i);
        if i < n {
            if !(0x20..0x7f).contains(&c) {
                return false;
            }
        } else if c != 0 {
            return false;
        }
        i += 1;
    }
    true
}

unsafe fn copy_text(dst: *mut u8, src: *const u8, n: usize) {
    let mut i = 0;
    while i < n {
        *dst.add(i) = *src.add(i);
        i += 1;
    }
}

/// The record counts of the section: parameters, machines, pages, actions, native pages.
unsafe fn counts_of(reg: *const u8) -> (u32, u32, u32, u32, u32) {
    let (mut np, mut nm, mut ng, mut na, mut nn) = (0u32, 0u32, 0u32, 0u32, 0u32);
    each(reg, |kind, _| {
        if kind == KIND_PARAM {
            np += 1;
        } else if kind == KIND_MACHINE {
            nm += 1;
        } else if kind == KIND_ACTION {
            na += 1;
        } else if nativepage::is_kind(kind) {
            nn += 1;
        } else if kind == KIND_PAGE {
            ng += 1;
        }
    });
    (np, nm, ng, na, nn)
}

/// The most rows of each kind the registry has.
#[allow(unused_variables)]
unsafe fn ck_counts(c: &Ctx) -> i32 {
    let (reg, image_size, inf) = (c.reg, c.image_size, c.inf);
    let (np, nm, ng, na, nn) = counts_of(reg);
    if np as usize > MAX_PARAM || nm as usize > MAX_MACH || ng as usize > MAX_PAGE || nn > 2 * nm {
        return E_FORMAT;
    }
    OK
}

/// An action names a parameter of the file and code inside the image; a parameter has at most one.
#[allow(unused_variables)]
unsafe fn ck_actions(c: &Ctx) -> i32 {
    let (reg, image_size, inf) = (c.reg, c.image_size, c.inf);
    let (np, nm, ng, na, nn) = counts_of(reg);
    // an action names a parameter of the file and code inside the image; a parameter has at most one
    let mut acted = 0u32;
    let mut bad_act = OK;
    each(reg, |kind, b| {
        if kind == KIND_ACTION && bad_act == OK {
            let (pi, off) = (rd32(b), rd32(b.add(4)));
            if pi >= np || pi >= 32 || acted & (1 << pi) != 0 || off >= image_size {
                bad_act = E_FORMAT;
            } else {
                acted |= 1 << pi;
            }
        }
    });
    if bad_act != OK || na > np {
        return E_FORMAT;
    }
    OK
}

/// Native pages: each record, the claimed pages, the bindings against the machine's parameters.
#[allow(unused_variables)]
unsafe fn ck_native<C: CoreApi>(core: &C, c: &Ctx) -> i32 {
    let (reg, image_size, inf) = (c.reg, c.image_size, c.inf);
    let (np, nm, ng, na, nn) = counts_of(reg);
    let mut native_set = nativepage::PageSet::<MAX_MACH>::new();
    let mut bad_native = OK;
    each(reg, |kind, b| {
        if !nativepage::is_kind(kind) || bad_native != OK {
            return;
        }
        bad_native = nativepage::check(core, kind, b, np, image_size);
        if bad_native != OK {
            return;
        }
        let owner = rd32(b.add(4));
        if !native_set.claim(kind, owner, nativepage::page_index(kind, b)) {
            bad_native = E_FORMAT;
            return;
        }
        let mut machine = false;
        let mut ordinary_pages = 0;
        each(reg, |k, row| {
            if k == KIND_MACHINE && rd32(row) == owner {
                machine = true;
            }
            if k == KIND_PAGE && rd32(row) == owner {
                ordinary_pages += 1;
            }
        });
        if !machine || (iw(inf, w::PG_ROWS) != 0 && ordinary_pages != 1) {
            bad_native = E_FORMAT;
            return;
        }
        let mut snapshot = [false; MAX_PARAM];
        each(reg, |k, row| {
            if (k == KIND_MACHINE || k == KIND_PAGE) && rd32(row) == owner {
                let offset = if k == KIND_MACHINE { 20 } else { 4 };
                for j in 0..8 {
                    let code = rd32(row.add(offset + 4 * j));
                    if code & KNOB_PARAM != 0 {
                        let index = (code & !KNOB_PARAM) as usize;
                        if index < MAX_PARAM {
                            snapshot[index] = true;
                        }
                    }
                }
            }
        });
        if snapshot.iter().filter(|&&used| used).count() > 8 {
            bad_native = E_FORMAT;
            return;
        }
        let mut i = 0;
        while i < 8 && bad_native == OK {
            let binding = b.add(nativepage::binding_offset(kind) + 12 * i);
            if rd32(binding) == 1 {
                let param = KNOB_PARAM | rd32(binding.add(4));
                let mut owned = false;
                each(reg, |k, row| {
                    if (k == KIND_MACHINE || k == KIND_PAGE) && rd32(row) == owner {
                        let offset = if k == KIND_MACHINE { 20 } else { 4 };
                        let mut j = 0;
                        while j < 8 {
                            if rd32(row.add(offset + 4 * j)) == param {
                                owned = true;
                            }
                            j += 1;
                        }
                    }
                });
                if !owned {
                    bad_native = E_FORMAT;
                }
            }
            i += 1;
        }
    });
    if bad_native != OK {
        return bad_native;
    }
    if !native_set.complete() {
        return E_FORMAT;
    }
    OK
}

/// Pages need a device that has page rows.
#[allow(unused_variables)]
unsafe fn ck_device(c: &Ctx) -> i32 {
    let (reg, image_size, inf) = (c.reg, c.image_size, c.inf);
    let (np, nm, ng, na, nn) = counts_of(reg);
    // pages need a device that has page rows
    let has_pages = iw(inf, w::PG_ROWS) != 0;
    if ng != 0 && !has_pages {
        return E_FORMAT;
    }
    OK
}

/// The parameter, page and machine records, in file order.
#[allow(unused_variables)]
unsafe fn ck_records<C: CoreApi>(core: &C, c: &Ctx) -> i32 {
    let (reg, image_size, inf) = (c.reg, c.image_size, c.inf);
    let (np, nm, ng, na, nn) = counts_of(reg);
    let s = rstate();
    // pass 2: the records
    let has_pages = iw(inf, w::PG_ROWS) != 0;
    let (p_first, knobs, slot_base) = (
        iw(inf, w::P_FIRST),
        iw(inf, w::KNOBS),
        iw(inf, w::SLOT_BASE),
    );
    let (dyn0, dyn_n, m_first) = (iw(inf, w::M_DYN0), iw(inf, w::M_DYN_N), iw(inf, w::M_FIRST));
    let mut seen_types = [0u32; MAX_MACH];
    let mut ns = 0usize;
    let mut bad = OK;
    let mut pi = 0u32;
    each(reg, |kind, b| {
        if bad != OK || !(kind == KIND_PARAM || kind == KIND_PAGE || kind == KIND_MACHINE) {
            return; // an action and a native page were checked above, an effect belongs to another service: none has a machine, parameter or page row of this registry
        }
        if kind == KIND_PARAM {
            let index = rd32(b);
            let (template, slot) = (rd32(b.add(4)), rd32(b.add(8)));
            let (min, max) = (rd32(b.add(12)) as i32, rd32(b.add(16)) as i32);
            if index != pi
                || template >= p_first
                || slot < slot_base
                || slot >= slot_base + knobs
                || min > max
                || !text_ok(b.add(24), 32)
                || !text_ok(b.add(56), 16)
            {
                bad = E_FORMAT;
            }
            pi += 1;
        } else if kind == KIND_PAGE {
            // machine type (checked against the file's machines below), eight knobs, level, name, title
            let mut j = 0;
            while j < KNOBS {
                let kn = rd32(b.add(4 + 4 * j));
                if kn != 0 && (kn & KNOB_PARAM == 0 || (kn & !KNOB_PARAM) >= np) {
                    bad = E_FORMAT;
                    return;
                }
                j += 1;
            }
            if rd32(b.add(36)) > 0xffff
                || !text_ok(b.add(40), MACH_TEXT)
                || !text_ok(b.add(40 + MACH_TEXT), MACH_TEXT)
                || knobs as usize != KNOBS
            {
                bad = E_FORMAT;
            }
        } else {
            // the first word is a machine key; `assign` has the number for it (0: none, which is out of range below)
            let t = crate::assign::resolve(core, rd32(b));
            let clone = rd32(b.add(4));
            let own = rd32(b.add(12));
            if rd32(b.add(8)) != 255
                || own > 1
                || (own == 1 && !has_pages)
                || rd32(b.add(16)) > 0xffff
            {
                bad = E_FORMAT;
                return;
            }
            let r = capacity::machine_type(t, dyn0, dyn_n, clone, m_first);
            if r != OK {
                bad = r;
                return;
            }
            let mut j = 0;
            while j < ns {
                if seen_types[j] == t {
                    bad = E_FORMAT;
                    return;
                }
                j += 1;
            }
            seen_types[ns] = t;
            ns += 1;
            let k = (t - dyn0) as usize;
            let row = iw(inf, w::M_ROWS) + (t - m_first) * iw(inf, w::M_ROW_SIZE);
            let name = rd(row + iw(inf, w::O_NAME));
            if s.mach_owner[k] != 0 || (name != 0 && *(name as *const u8) != 0) {
                bad = E_TYPE_USED;
                return;
            }
            let mut j = 0;
            while j < knobs as usize {
                let kn = rd32(b.add(20 + 4 * j));
                if kn & KNOB_PARAM != 0 {
                    if (kn & !KNOB_PARAM) >= np {
                        bad = E_FORMAT;
                        return;
                    }
                } else if kn >= p_first {
                    bad = E_FORMAT;
                    return;
                }
                j += 1;
            }
            if !text_ok(b.add(52), MACH_TEXT) || !text_ok(b.add(52 + MACH_TEXT), MACH_TEXT) {
                bad = E_FORMAT;
            }
        }
    });
    if bad != OK {
        return bad;
    }
    OK
}

/// A page belongs to a machine of the same file.
#[allow(unused_variables)]
unsafe fn ck_owners<C: CoreApi>(core: &C, c: &Ctx) -> i32 {
    let (reg, image_size, inf) = (c.reg, c.image_size, c.inf);
    let (np, nm, ng, na, nn) = counts_of(reg);
    // a page belongs to a machine of the same file
    let mut bad = OK;
    each(reg, |kind, b| {
        if kind != KIND_PAGE || bad != OK {
            return;
        }
        let t = crate::assign::resolve(core, rd32(b));
        let mut found = false;
        each(reg, |k, m| {
            found |= k == KIND_MACHINE && crate::assign::resolve(core, rd32(m)) == t;
        });
        if !found {
            bad = E_FORMAT;
        }
    });
    if bad != OK {
        return bad;
    }
    OK
}

/// Free reserved page and parameter rows.
#[allow(unused_variables)]
unsafe fn ck_rows<C: CoreApi>(core: &C, c: &Ctx) -> i32 {
    let (reg, image_size, inf) = (c.reg, c.image_size, c.inf);
    let (np, nm, ng, na, nn) = counts_of(reg);
    let s = rstate();
    // free reserved page and parameter rows
    return capacity::registry_rows(
        ng,
        capacity::free_rows(&s.page_owner, iw(inf, w::PG_DYN_N)),
        np,
        capacity::free_rows(&s.param_owner, iw(inf, w::P_DYN_N)),
    );
}

/// Fill the reserved parameter, action, page and machine rows from the (checked) records.
unsafe fn apply<C: CoreApi>(core: &C, ctx: &AppCtx) -> i32 {
    let (owner, reg, base, inf, args) =
        (ctx.owner, ctx.reg, ctx.base, ctx.inf, crate::build::args());
    let s = rstate();
    // parameters first: their codes are what the machines' knobs name
    let mut codes = [0u32; MAX_PARAM];
    let mut rows = [0usize; MAX_PARAM]; // the file's parameter i sits in row rows[i]
    let mut np = 0usize;
    each(reg, |kind, b| {
        if kind != KIND_PARAM {
            return;
        }
        let mut k = 0;
        while k < MAX_PARAM && s.param_owner[k] != 0 {
            k += 1;
        }
        if k == MAX_PARAM {
            return; // cannot happen after `check`
        }
        s.param_owner[k] = owner;
        let code = iw(inf, w::P_DYN0) + k as u32;
        codes[np] = code;
        rows[np] = k;
        np += 1;
        copy_text(s.s.param_name[k].as_mut_ptr(), b.add(24), 32);
        copy_text(s.s.param_short[k].as_mut_ptr(), b.add(56), 16);
        dkrt_svc_params::write_param_row(
            core,
            inf,
            code,
            rd32(b.add(4)),
            rd32(b.add(8)),
            rd32(b.add(12)),
            rd32(b.add(16)),
            rd32(b.add(20)),
            s.s.param_name[k].as_ptr() as u32,
            s.s.param_short[k].as_ptr() as u32,
        );
    });
    // click actions: the callback's address is the module's load address plus its image offset
    let mut ai = 0usize;
    each(reg, |kind, b| {
        if kind == KIND_ACTION {
            slots::set(rows[rd32(b) as usize], base + rd32(b.add(4)));
            if let Some(a) = args {
                slots::set_arg(rows[rd32(b) as usize], a.click[ai % 32]);
            }
            ai += 1;
        }
    });
    // pages (Digitone II): their machine is published below, so they sit unused until then
    let slot_base = iw(inf, w::SLOT_BASE);
    each(reg, |kind, b| {
        if kind != KIND_PAGE {
            return;
        }
        let mut k = 0;
        while k < MAX_PAGE && s.page_owner[k] != 0 {
            k += 1;
        }
        if k == MAX_PAGE {
            return; // cannot happen after `check`
        }
        s.page_owner[k] = owner;
        let row = iw(inf, w::PG_ROWS)
            + (iw(inf, w::PG_DYN0) + k as u32 - iw(inf, w::PG_FIRST)) * iw(inf, w::PG_ROW_SIZE);
        copy_text(s.s.page_name[k].as_mut_ptr(), b.add(40), MACH_TEXT);
        copy_text(
            s.s.page_title[k].as_mut_ptr(),
            b.add(40 + MACH_TEXT),
            MACH_TEXT,
        );
        let name = dkrt_svc_params::make_rep(
            s.s.rep_pname[k].as_mut_ptr(),
            s.s.page_name[k].as_ptr(),
            false,
        );
        let title = dkrt_svc_params::make_rep(
            s.s.rep_ptitle[k].as_mut_ptr(),
            s.s.page_title[k].as_ptr(),
            false,
        );
        let (c0, s0) = (row + iw(inf, w::PGO_C), row + iw(inf, w::PGO_S));
        let mut j = 0;
        while j < KNOBS {
            let kn = rd32(b.add(4 + 4 * j));
            let known = kn & KNOB_PARAM != 0;
            wr(
                c0 + 4 * j as u32,
                if known {
                    codes[(kn & !KNOB_PARAM) as usize]
                } else {
                    0
                },
            );
            wr(
                s0 + 4 * j as u32,
                if known { slot_base + j as u32 } else { 0 },
            );
            j += 1;
        }
        wr(c0 + 4 * KNOBS as u32, rd32(b.add(36))); // the level code
        wr(s0 + 4 * KNOBS as u32, 0);
        wr(row + iw(inf, w::PGO_NAME), name);
        wr(row + iw(inf, w::PGO_TITLE), title);
        wr(
            row + iw(inf, w::PGO_MACH),
            crate::assign::resolve(core, rd32(b)),
        ); // the page belongs to its machine from this store on
    });
    // machines
    let knobs = iw(inf, w::KNOBS) as usize;
    each(reg, |kind, b| {
        if kind != KIND_MACHINE {
            return;
        }
        let t = crate::assign::resolve(core, rd32(b));
        let clone = rd32(b.add(4));
        let k = (t - iw(inf, w::M_DYN0)) as usize;
        let row = iw(inf, w::M_ROWS) + (t - iw(inf, w::M_FIRST)) * iw(inf, w::M_ROW_SIZE);
        let f0 = row + iw(inf, w::O_F0);
        let mut j = 0;
        while j < knobs {
            let kn = rd32(b.add(20 + 4 * j));
            wr(
                f0 + 4 * j as u32,
                if kn & KNOB_PARAM != 0 {
                    codes[(kn & !KNOB_PARAM) as usize]
                } else {
                    kn
                },
            );
            j += 1;
        }
        wr(f0 + 4 * knobs as u32, rd32(b.add(16))); // the level, the field after the knobs
        wr(row + iw(inf, w::O_CLONE), clone);
        wr(row + iw(inf, w::O_POS), rd32(b.add(8)));
        wr(row + iw(inf, w::O_OWN), rd32(b.add(12)));
        // strings
        copy_text(s.s.mach_name[k].as_mut_ptr(), b.add(52), MACH_TEXT);
        copy_text(
            s.s.mach_short[k].as_mut_ptr(),
            b.add(52 + MACH_TEXT),
            MACH_TEXT,
        );
        let long = dkrt_svc_params::make_rep(
            s.s.rep_long[k].as_mut_ptr(),
            s.s.mach_name[k].as_ptr(),
            true,
        );
        let short = dkrt_svc_params::make_rep(
            s.s.rep_short[k].as_mut_ptr(),
            s.s.mach_short[k].as_ptr(),
            true,
        );
        wr(row + iw(inf, w::O_LONG), long);
        wr(row + iw(inf, w::O_SHORT), short);
        wr(row + iw(inf, w::O_SNAME), s.s.mach_short[k].as_ptr() as u32);
        // the per-type tables: the clone's row, our names
        let nrow = iw(inf, w::NAMES) + 12 * t;
        wr(nrow, s.s.mach_name[k].as_ptr() as u32);
        wr(nrow + 4, s.s.mach_short[k].as_ptr() as u32);
        wr(nrow + 8, rd(iw(inf, w::NAMES) + 12 * clone + 8));
        wr(
            iw(inf, w::PERMIT) + 4 * t,
            rd(iw(inf, w::PERMIT) + 4 * clone),
        );
        let pt = iw(inf, w::PERTYPE);
        if pt != 0 {
            *((pt + t) as *mut u8) = *((pt + clone) as *const u8);
        }
        // publish: the name pointer marks the row as used (one aligned store)
        s.mach_owner[k] = owner;
        wr(row + iw(inf, w::O_NAME), s.s.mach_name[k].as_ptr() as u32);
    });
    // what the native pages need when the walker reaches them in file order
    let st = &mut *addr_of_mut!(APPLIED);
    st.codes = codes;
    st.np = np;
    st.ni = 0;
    OK
}

/// The parameter codes of the records `apply` has written and the native pages seen so far: state between `apply` and `apply_record`
/// of one section (the registry applies one module at a time, in the UI task).
struct Applied {
    codes: [u32; MAX_PARAM],
    np: usize,
    ni: usize,
}
static mut APPLIED: Applied = Applied {
    codes: [0; MAX_PARAM],
    np: 0,
    ni: 0,
};

/// A native page record, in file order after every machine row is in: its callbacks are registered. Other kinds: OK.
unsafe fn apply_record<C: CoreApi>(core: &C, kind: u32, b: *const u8, c: &ApplyCtx) -> i32 {
    if !nativepage::is_kind(kind) {
        return OK;
    }
    let (reg, base, args) = (c.reg, c.base, crate::build::args());
    let st = &mut *addr_of_mut!(APPLIED);
    let (codes, np, ni) = (&st.codes, st.np, st.ni);
    let mut rc = OK;
    // Snapshot only this machine's existing parameters, independently of
    // its page2 encoder bindings. Registry order is declaration order.
    let machine = rd32(b.add(4));
    let mut words = [0u32; 8];
    let mut count = 0;
    for (index, code) in codes[..np].iter().enumerate() {
        let mut owned = false;
        each(reg, |k, row| {
            if (k == KIND_MACHINE || k == KIND_PAGE) && rd32(row) == machine {
                let offset = if k == KIND_MACHINE { 20 } else { 4 };
                for j in 0..8 {
                    if rd32(row.add(offset + 4 * j)) == (KNOB_PARAM | index as u32) {
                        owned = true;
                    }
                }
            }
        });
        if owned {
            if count == words.len() {
                rc = E_FORMAT;
                break;
            }
            words[count] = *code;
            count += 1;
        }
    }
    if rc == OK {
        rc = nativepage::apply(
            core,
            kind,
            b,
            base,
            codes,
            &words[..count],
            args.map(|a| a.native[ni % 8]),
        );
    }
    st.ni += 1;
    rc
}

/// Give back the machine rows `owner` filled (hidden, then reset to a gap row), then its pages, then its parameters.
unsafe fn remove<C: CoreApi>(core: &C, owner: u8) {
    let inf = dkrt_svc_params::dynreg(core);
    if inf.is_null() {
        return;
    }
    let s = rstate();
    let empty = iw(inf, w::EMPTY);
    let knobs = iw(inf, w::KNOBS) as usize;
    let first = iw(inf, w::M_FIRST);
    let mut k = 0;
    while k < MAX_MACH {
        if s.mach_owner[k] == owner {
            let t = iw(inf, w::M_DYN0) + k as u32;
            nativepage::remove(core, t); // callbacks must go before code/strings can be released
            let row = iw(inf, w::M_ROWS) + (t - first) * iw(inf, w::M_ROW_SIZE);
            wr(row + iw(inf, w::O_NAME), empty); // hide first
            s.empty_rep = [0, 0, 0xffff_ffff, 0];
            let rep = addr_of!(rstate().empty_rep) as u32 + 12;
            wr(row + iw(inf, w::O_LONG), rep);
            wr(row + iw(inf, w::O_SHORT), rep);
            wr(row + iw(inf, w::O_SNAME), empty);
            let f0 = row + iw(inf, w::O_F0);
            let mut j = 0;
            while j < knobs {
                wr(f0 + 4 * j as u32, 0);
                j += 1;
            }
            wr(f0 + 4 * knobs as u32, 10);
            wr(row + iw(inf, w::O_CLONE), 0);
            wr(row + iw(inf, w::O_OWN), 0);
            let nrow = iw(inf, w::NAMES) + 12 * t;
            wr(nrow, empty);
            wr(nrow + 4, empty);
            wr(nrow + 8, rd(iw(inf, w::NAMES) + 8));
            wr(iw(inf, w::PERMIT) + 4 * t, rd(iw(inf, w::PERMIT)));
            let pt = iw(inf, w::PERTYPE);
            if pt != 0 {
                *((pt + t) as *mut u8) = *(pt as *const u8);
            }
            s.mach_owner[k] = 0;
        }
        k += 1;
    }
    // pages: unbind from the machine first, then the rest of the row back to a free row
    k = 0;
    while k < MAX_PAGE {
        if s.page_owner[k] == owner {
            let row = iw(inf, w::PG_ROWS)
                + (iw(inf, w::PG_DYN0) + k as u32 - iw(inf, w::PG_FIRST)) * iw(inf, w::PG_ROW_SIZE);
            wr(row + iw(inf, w::PGO_MACH), iw(inf, w::PG_FREE));
            s.empty_rep = [0, 0, 0xffff_ffff, 0];
            let rep = addr_of!(rstate().empty_rep) as u32 + 12;
            wr(row + iw(inf, w::PGO_NAME), rep);
            wr(row + iw(inf, w::PGO_TITLE), rep);
            let (c0, s0) = (row + iw(inf, w::PGO_C), row + iw(inf, w::PGO_S));
            let mut j = 0;
            while j < KNOBS {
                wr(c0 + 4 * j as u32, 0);
                wr(s0 + 4 * j as u32, 0);
                j += 1;
            }
            wr(c0 + 4 * KNOBS as u32, 10); // the track level code, as in a free row
            wr(s0 + 4 * KNOBS as u32, 0);
            s.page_owner[k] = 0;
        }
        k += 1;
    }
    k = 0;
    while k < MAX_PARAM {
        if s.param_owner[k] == owner {
            dkrt_svc_params::write_param_row(
                core,
                inf,
                iw(inf, w::P_DYN0) + k as u32,
                iw(inf, w::RESERVE_TEMPLATE),
                iw(inf, w::SLOT_BASE),
                0,
                0,
                0,
                empty,
                empty,
            );
            s.param_owner[k] = 0;
            slots::set(k, 0);
        }
        k += 1;
    }
}

/// The machine type numbers of the machine rows `owner` filled (at most 8), in type order; returns how many.
pub fn types_of(owner: u8, out: &mut [u8; MAX_MACH]) -> usize {
    let s = rstate();
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

#[cfg(test)]
mod tests {
    //! The checks of a `register` section that read no firmware address: the counts, the actions, the device, the records up to the first machine row, and the owners.
    //! A machine record that passes its own checks reads its row in the firmware's table, which a host cannot reach (the device tests cover that), so the machine cases
    //! here are the ones that stop before.
    use super::*;
    use dkrt_test_support::FakeCore;
    use std::vec::Vec;

    type Rec = (u32, Vec<u8>);

    /// A section of `recs`: the 16-byte header with the count at 12, then each record as kind, total length and body.
    fn section(recs: &[Rec]) -> Vec<u8> {
        let mut s = std::vec![0u8; 16];
        s[12..16].copy_from_slice(&(recs.len() as u32).to_be_bytes());
        for (kind, body) in recs {
            s.extend_from_slice(&kind.to_be_bytes());
            s.extend_from_slice(&(8 + body.len() as u32).to_be_bytes());
            s.extend_from_slice(body);
        }
        s
    }

    /// `n` records of `kind` with an empty body (only their kinds are counted).
    fn kinds(kind: u32, n: usize) -> Vec<Rec> {
        std::vec![(kind, std::vec![]); n]
    }

    /// The words of a `dynreg_info` table: the device has page rows when `pages`; the first parameter code, knob count and slot base of these tests.
    fn info(pages: bool) -> Vec<u32> {
        let mut words = std::vec![0u32; 128];
        words[w::PG_ROWS] = pages as u32;
        words[w::P_FIRST] = 100;
        words[w::KNOBS] = KNOBS as u32;
        words[w::SLOT_BASE] = 10;
        words[w::M_FIRST] = 7;
        words[w::M_DYN0] = 16;
        words[w::M_DYN_N] = 8;
        words
    }

    fn put(b: &mut [u8], at: usize, v: u32) {
        b[at..at + 4].copy_from_slice(&v.to_be_bytes());
    }

    fn text(b: &mut [u8], at: usize, field: usize, s: &[u8]) {
        b[at..at + field].fill(0);
        b[at..at + s.len()].copy_from_slice(s);
    }

    fn check(f: unsafe fn(&Ctx) -> i32, recs: &[Rec], pages: bool) -> i32 {
        let (s, inf) = (section(recs), info(pages));
        unsafe {
            f(&Ctx {
                reg: s.as_ptr(),
                image_size: 100,
                inf: inf.as_ptr(),
            })
        }
    }

    fn check_core(f: unsafe fn(&FakeCore, &Ctx) -> i32, recs: &[Rec], pages: bool) -> i32 {
        check_core_with(f, recs, info(pages))
    }

    fn check_core_with(f: unsafe fn(&FakeCore, &Ctx) -> i32, recs: &[Rec], inf: Vec<u32>) -> i32 {
        let (s, core) = (section(recs), FakeCore::new());
        unsafe {
            f(
                &core,
                &Ctx {
                    reg: s.as_ptr(),
                    image_size: 100,
                    inf: inf.as_ptr(),
                },
            )
        }
    }

    #[test]
    fn a_section_may_hold_the_most_rows_of_each_kind_the_registry_has_and_native_pages_for_its_machines_only(
    ) {
        for (kind, most) in [
            (KIND_PARAM, MAX_PARAM),
            (KIND_MACHINE, MAX_MACH),
            (KIND_PAGE, MAX_PAGE),
        ] {
            assert_eq!(
                check(ck_counts, &kinds(kind, most), true),
                OK,
                "{most} of kind {kind}"
            );
            assert_eq!(
                check(ck_counts, &kinds(kind, most + 1), true),
                E_FORMAT,
                "{} of kind {kind}",
                most + 1
            );
        }
        // a machine has at most two native pages (the two kinds), so three for one machine are refused
        let with_native = |n| [kinds(KIND_MACHINE, 1), kinds(KIND_NATIVE_PAGE, n)].concat();
        assert_eq!(check(ck_counts, &with_native(2), true), OK);
        assert_eq!(check(ck_counts, &with_native(3), true), E_FORMAT);
        assert_eq!(check(ck_counts, &[], true), OK);
    }

    fn action(parameter: u32, offset: u32) -> Rec {
        let mut b = std::vec![0u8; LEN_ACTION as usize];
        put(&mut b, 0, parameter);
        put(&mut b, 4, offset);
        (KIND_ACTION, b)
    }

    #[test]
    fn an_action_names_a_parameter_of_the_file_once_and_code_inside_the_image() {
        let params = |n| kinds(KIND_PARAM, n);
        let with = |n, acts: &[Rec]| [params(n), acts.to_vec()].concat();
        assert_eq!(check(ck_actions, &with(1, &[action(0, 99)]), true), OK);
        // an offset at the image size is outside it (the image of these checks is 100 bytes)
        assert_eq!(
            check(ck_actions, &with(1, &[action(0, 100)]), true),
            E_FORMAT
        );
        // a parameter the file does not have, and one parameter with two actions
        assert_eq!(check(ck_actions, &with(1, &[action(1, 0)]), true), E_FORMAT);
        assert_eq!(
            check(ck_actions, &with(2, &[action(0, 0), action(0, 4)]), true),
            E_FORMAT
        );
        assert_eq!(
            check(ck_actions, &with(2, &[action(0, 0), action(1, 4)]), true),
            OK
        );
    }

    #[test]
    fn pages_need_a_device_that_has_page_rows() {
        let page = kinds(KIND_PAGE, 1);
        assert_eq!(check(ck_device, &page, false), E_FORMAT);
        assert_eq!(check(ck_device, &page, true), OK);
        assert_eq!(check(ck_device, &[], false), OK);
    }

    fn param(
        index: u32,
        template: u32,
        slot: u32,
        min: i32,
        max: i32,
        name: &[u8],
        short: &[u8],
    ) -> Rec {
        let mut b = std::vec![0u8; LEN_PARAM as usize];
        put(&mut b, 0, index);
        put(&mut b, 4, template);
        put(&mut b, 8, slot);
        put(&mut b, 12, min as u32);
        put(&mut b, 16, max as u32);
        text(&mut b, 24, 32, name);
        text(&mut b, 56, 16, short);
        (KIND_PARAM, b)
    }

    fn good_param(index: u32) -> Rec {
        param(index, 5, 10, 0, 127, b"Level", b"LVL")
    }

    #[test]
    fn the_parameter_records_are_in_order_with_a_template_a_slot_a_range_and_texts() {
        let one = |r: Rec| check_core(ck_records, &[r], false);
        assert_eq!(
            check_core(ck_records, &[good_param(0), good_param(1)], false),
            OK
        );
        // the index is the position among the parameters
        assert_eq!(one(good_param(1)), E_FORMAT);
        assert_eq!(
            check_core(ck_records, &[good_param(0), good_param(0)], false),
            E_FORMAT
        );
        // the template is a row before the first of the run-time parameters; the slot is one of the page's knobs
        assert_eq!(one(param(0, 100, 10, 0, 127, b"A", b"A")), E_FORMAT);
        assert_eq!(one(param(0, 99, 10, 0, 127, b"A", b"A")), OK);
        assert_eq!(one(param(0, 5, 9, 0, 127, b"A", b"A")), E_FORMAT);
        assert_eq!(
            one(param(0, 5, 10 + KNOBS as u32, 0, 127, b"A", b"A")),
            E_FORMAT
        );
        assert_eq!(one(param(0, 5, 9 + KNOBS as u32, 0, 127, b"A", b"A")), OK);
        // min over max; the range of one value is fine
        assert_eq!(one(param(0, 5, 10, 1, 0, b"A", b"A")), E_FORMAT);
        assert_eq!(one(param(0, 5, 10, 3, 3, b"A", b"A")), OK);
        assert_eq!(one(param(0, 5, 10, -5, 5, b"A", b"A")), OK);
        // the long name is 1 to 31 characters, the short name 1 to 15
        assert_eq!(one(param(0, 5, 10, 0, 1, b"", b"A")), E_FORMAT);
        assert_eq!(one(param(0, 5, 10, 0, 1, &[b'n'; 32], b"A")), E_FORMAT);
        assert_eq!(one(param(0, 5, 10, 0, 1, &[b'n'; 31], b"A")), OK);
        assert_eq!(one(param(0, 5, 10, 0, 1, b"A", b"")), E_FORMAT);
        assert_eq!(one(param(0, 5, 10, 0, 1, b"A", &[b's'; 16])), E_FORMAT);
        assert_eq!(one(param(0, 5, 10, 0, 1, b"A", &[b's'; 15])), OK);
        assert_eq!(one(param(0, 5, 10, 0, 1, b"A\x07", b"A")), E_FORMAT);
    }

    fn page(machine_key: u32, knobs: [u32; 8], level: u32, name: &[u8], title: &[u8]) -> Rec {
        let mut b = std::vec![0u8; LEN_PAGE as usize];
        put(&mut b, 0, machine_key);
        for (j, k) in knobs.iter().enumerate() {
            put(&mut b, 4 + 4 * j, *k);
        }
        put(&mut b, 36, level);
        text(&mut b, 40, MACH_TEXT, name);
        text(&mut b, 40 + MACH_TEXT, MACH_TEXT, title);
        (KIND_PAGE, b)
    }

    #[test]
    fn a_page_knob_names_a_parameter_of_the_file_and_the_page_has_a_level_and_texts() {
        let no_knobs = [0u32; 8];
        let with_knob = |k: u32| {
            let mut knobs = no_knobs;
            knobs[2] = k;
            check_core(
                ck_records,
                &[good_param(0), page(0x10, knobs, 0, b"Name", b"Title")],
                true,
            )
        };
        assert_eq!(with_knob(0), OK, "no knob at all in the slot is fine");
        assert_eq!(with_knob(KNOB_PARAM), OK, "parameter 0 of the file");
        assert_eq!(
            with_knob(KNOB_PARAM | 1),
            E_FORMAT,
            "the file has one parameter"
        );
        assert_eq!(
            with_knob(5),
            E_FORMAT,
            "a knob that is not a parameter of the file"
        );
        let one = |p: Rec| check_core(ck_records, &[p], true);
        assert_eq!(one(page(0x10, no_knobs, 0xffff, b"N", b"T")), OK);
        assert_eq!(one(page(0x10, no_knobs, 0x1_0000, b"N", b"T")), E_FORMAT);
        assert_eq!(one(page(0x10, no_knobs, 0, b"", b"T")), E_FORMAT);
        assert_eq!(one(page(0x10, no_knobs, 0, b"N", b"")), E_FORMAT);
        assert_eq!(
            one(page(0x10, no_knobs, 0, &[b'n'; MACH_TEXT], b"T")),
            E_FORMAT
        );
        // a table that has another knob count than the build refuses every page
        let mut inf = info(true);
        inf[w::KNOBS] = KNOBS as u32 - 1;
        assert_eq!(
            check_core_with(ck_records, &[page(0x10, no_knobs, 0, b"N", b"T")], inf),
            E_FORMAT
        );
    }

    /// A machine record of the type key `key` that clones `clone` with the constant word and the own-page flag as given: the raw words that `ck_records` checks first.
    fn machine(key: u32, clone: u32, constant: u32, own: u32, last: u32) -> Rec {
        let mut b = std::vec![0u8; LEN_MACHINE as usize];
        put(&mut b, 0, key);
        put(&mut b, 4, clone);
        put(&mut b, 8, constant);
        put(&mut b, 12, own);
        put(&mut b, 16, last);
        text(&mut b, 52, MACH_TEXT, b"Machine");
        text(&mut b, 52 + MACH_TEXT, MACH_TEXT, b"MCH");
        (KIND_MACHINE, b)
    }

    #[test]
    fn a_machine_record_is_refused_for_its_raw_words_and_for_a_type_outside_the_run_time_range() {
        let one = |r: Rec, pages: bool| check_core(ck_records, &[r], pages);
        // the run-time types are 16 to 23 and the stock machines are the types below 7 (the table of these tests)
        assert_eq!(
            one(machine(0x10, 0, 254, 0, 0), true),
            E_FORMAT,
            "the constant word is 255"
        );
        assert_eq!(
            one(machine(0x10, 0, 255, 2, 0), true),
            E_FORMAT,
            "the own-page flag is 0 or 1"
        );
        assert_eq!(
            one(machine(0x10, 0, 255, 1, 0), false),
            E_FORMAT,
            "an own page needs a device with page rows"
        );
        assert_eq!(one(machine(0x10, 0, 255, 0, 0x1_0000), true), E_FORMAT);
        // the key resolves to a type below the run-time range, past it, and the clone source is not a stock machine
        assert_eq!(one(machine(0x07, 0, 255, 0, 0), true), E_TYPE_RANGE);
        assert_eq!(
            one(machine(0x00, 0, 255, 0, 0), true),
            E_TYPE_RANGE,
            "an unresolved key"
        );
        assert_eq!(one(machine(0x18, 0, 255, 0, 0), true), E_TYPE_RANGE);
        assert_eq!(one(machine(0x10, 7, 255, 0, 0), true), E_TYPE_RANGE);
    }

    #[test]
    fn a_page_belongs_to_a_machine_of_the_same_file() {
        let machines = [machine(0x10, 0, 255, 0, 0)];
        let pg = |key: u32| page(key, [0; 8], 0, b"N", b"T");
        assert_eq!(
            check_core(ck_owners, &[machines[0].clone(), pg(0x10)], true),
            OK
        );
        assert_eq!(
            check_core(ck_owners, &[machines[0].clone(), pg(0x11)], true),
            E_FORMAT
        );
        assert_eq!(check_core(ck_owners, &[pg(0x10)], true), E_FORMAT);
        // the machine may come after its page; a file with no page has nothing to own
        assert_eq!(
            check_core(ck_owners, &[pg(0x10), machines[0].clone()], true),
            OK
        );
        assert_eq!(check_core(ck_owners, &machines, true), OK);
    }
}
