//! `apply` and `apply_record`: fill the reserved parameter, action, page and machine rows from the checked records of a module.

use crate::parking::{LOGGED, MISSING_ROWS};
use crate::registry::{copy_text, machine_text, rstate, KNOBS, KNOB_PARAM, MAX_PAGE, MAX_PARAM};
use crate::{nativepage, slots};
use core::ptr::addr_of_mut;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::mem::{iw, rd32};
use dkrt_abi::registry::each;
use dkrt_abi::service::ApplyCtx;
use dkrt_sdk::records::{KIND_ACTION, KIND_MACHINE, KIND_PAGE, KIND_PARAM};
use dkrt_sdk::*;
use dkrt_svc_rows::dynreg::{w, MACH_TEXT};

/// What `apply` reads: the owner, the section, the load address and the words of `dynreg_info`.
pub(crate) struct AppCtx {
    pub(crate) owner: u8,
    pub(crate) reg: *const u8,
    pub(crate) base: u32,
    pub(crate) inf: *const u32,
}

/// Fill the reserved parameter, action, page and machine rows from the (checked) records.
pub(crate) unsafe fn apply<C: CoreApi>(core: &C, ctx: &AppCtx) -> i32 {
    let (owner, reg, base, inf, args) =
        (ctx.owner, ctx.reg, ctx.base, ctx.inf, crate::build::args());
    let s = rstate();
    // a machine whose key has no run-time type number (none assigned, or one outside the range) is refused before any row is written, as `ck_rows` does at load: the rows are
    // indexed by the number
    let mut refused = OK;
    each(reg, |kind, b| {
        if kind == KIND_MACHINE && refused == OK {
            let t = crate::assign::resolve(core, rd32(b));
            refused = capacity::machine_type(
                t,
                iw(inf, w::M_DYN0),
                iw(inf, w::M_DYN_N),
                rd32(b.add(4)),
                iw(inf, w::M_FIRST),
            );
        }
    });
    if refused != OK {
        return refused;
    }
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
        dkrt_svc_rows::write_param_row(
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
    // pages (on a device whose machine pages register before their machine): the machine is published below, so they sit unused until then
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
        let name = dkrt_svc_rows::make_rep(
            s.s.rep_pname[k].as_mut_ptr(),
            s.s.page_name[k].as_ptr(),
            false,
        );
        let title = dkrt_svc_rows::make_rep(
            s.s.rep_ptitle[k].as_mut_ptr(),
            s.s.page_title[k].as_ptr(),
            false,
        );
        let (c0, s0) = (row + iw(inf, w::PGO_C), row + iw(inf, w::PGO_S));
        let mut j = 0;
        while j < KNOBS {
            let kn = rd32(b.add(4 + 4 * j));
            let known = kn & KNOB_PARAM != 0;
            core.wr(
                c0 + 4 * j as u32,
                if known {
                    codes[(kn & !KNOB_PARAM) as usize]
                } else {
                    0
                },
            );
            core.wr(
                s0 + 4 * j as u32,
                if known { slot_base + j as u32 } else { 0 },
            );
            j += 1;
        }
        core.wr(c0 + 4 * KNOBS as u32, rd32(b.add(36))); // the level code
        core.wr(s0 + 4 * KNOBS as u32, 0);
        core.wr(row + iw(inf, w::PGO_NAME), name);
        core.wr(row + iw(inf, w::PGO_TITLE), title);
        core.wr(
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
            core.wr(
                f0 + 4 * j as u32,
                if kn & KNOB_PARAM != 0 {
                    codes[(kn & !KNOB_PARAM) as usize]
                } else {
                    kn
                },
            );
            j += 1;
        }
        core.wr(f0 + 4 * knobs as u32, rd32(b.add(16))); // the level, the field after the knobs
        core.wr(row + iw(inf, w::O_CLONE), clone);
        core.wr(row + iw(inf, w::O_POS), rd32(b.add(8)));
        core.wr(row + iw(inf, w::O_OWN), rd32(b.add(12)));
        // strings
        let (long, short) = machine_text(k, b.add(52), b.add(52 + MACH_TEXT), true);
        core.wr(row + iw(inf, w::O_LONG), long);
        core.wr(row + iw(inf, w::O_SHORT), short);
        core.wr(row + iw(inf, w::O_SNAME), s.s.mach_short[k].as_ptr() as u32);
        // the per-type tables: the clone's row, our names
        let nrow = iw(inf, w::NAMES) + 12 * t;
        core.wr(nrow, s.s.mach_name[k].as_ptr() as u32);
        core.wr(nrow + 4, s.s.mach_short[k].as_ptr() as u32);
        core.wr(nrow + 8, core.rd(iw(inf, w::NAMES) + 12 * clone + 8));
        core.wr(
            iw(inf, w::PERMIT) + 4 * t,
            core.rd(iw(inf, w::PERMIT) + 4 * clone),
        );
        let pt = iw(inf, w::PERTYPE);
        if pt != 0 {
            core.wr8(pt + t, core.rd8(pt + clone));
        }
        // publish: the name pointer marks the row as used (one aligned store)
        s.mach_owner[k] = owner;
        MISSING_ROWS &= !(1 << k);
        LOGGED &= !(1 << k);
        core.wr(row + iw(inf, w::O_NAME), s.s.mach_name[k].as_ptr() as u32);
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
pub(crate) unsafe fn apply_record<C: CoreApi>(
    core: &C,
    kind: u32,
    b: *const u8,
    c: &ApplyCtx,
) -> i32 {
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
