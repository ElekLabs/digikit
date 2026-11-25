//! The load check of a `register` section: the seven steps of `Records::check` (counts, actions, native pages, device, records, owners, rows).
use crate::native_page::{record, PageSet};
use crate::registry::{mach_owner, KNOBS, KNOB_PARAM, MAX_MACH, MAX_PAGE, MAX_PARAM};
use crate::validate::{body_len, Acc};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::mem::{iw, rd32};
use dkrt_abi::registry::each;
use dkrt_abi::service::Problem;
use dkrt_sdk::records::{KIND_ACTION, KIND_MACHINE, KIND_PAGE, KIND_PARAM};
use dkrt_sdk::*;
use dkrt_svc_rows::rows_info::{w, RegState};

/// What the checks read: the section, the words of `rows_info` and the owner's registry state.
pub(crate) struct Ctx<'a> {
    pub(crate) reg: *const u8,
    pub(crate) image_size: u32,
    pub(crate) inf: *const u32,
    pub(crate) state: &'a RegState,
}

/// The table must fit this registry's state (a table that says more rows than the state has would index past it).
pub(crate) unsafe fn fits(inf: *const u32) -> bool {
    !(iw(inf, w::M_DYN_N) as usize > MAX_MACH
        || iw(inf, w::P_DYN_N) as usize > MAX_PARAM
        || iw(inf, w::PG_DYN_N) as usize > MAX_PAGE)
}

/// The record counts of the section: parameters, machines, pages, actions, native pages.
pub(crate) unsafe fn counts_of(reg: *const u8) -> (u32, u32, u32, u32, u32) {
    let (mut np, mut nm, mut ng, mut na, mut nn) = (0u32, 0u32, 0u32, 0u32, 0u32);
    each(reg, |kind, _| {
        if kind == KIND_PARAM {
            np += 1;
        } else if kind == KIND_MACHINE {
            nm += 1;
        } else if kind == KIND_ACTION {
            na += 1;
        } else if record::is_kind(kind) {
            nn += 1;
        } else if kind == KIND_PAGE {
            ng += 1;
        }
    });
    (np, nm, ng, na, nn)
}

/// The records meet the rule of `validate::Acc` (row counts, sizes, fields, texts, a machine key and an action's parameter once), and the native pages are at most two a machine.
/// The first problem's code is the refusal.
pub(crate) unsafe fn ck_counts(c: &Ctx<'_>) -> i32 {
    let reg = c.reg;
    let mut acc = Acc::new();
    let mut first = OK;
    each(reg, |kind, b| {
        if let Some(len) = body_len(kind) {
            // SAFETY: the walker validated the record length of `kind`
            acc.record(
                kind,
                core::slice::from_raw_parts(b, len),
                &mut |p: Problem| {
                    if first == OK {
                        first = p.code;
                    }
                },
            );
        }
    });
    if first != OK {
        return first;
    }
    let (_, nm, _, _, nn) = counts_of(reg);
    if nn > 2 * nm {
        return E_FORMAT;
    }
    OK
}

/// An action names a parameter of the file and code inside the image (that a parameter has one action at most is the rule of `ck_counts`).
pub(crate) unsafe fn ck_actions(c: &Ctx<'_>) -> i32 {
    let (reg, image_size) = (c.reg, c.image_size);
    let (np, _, _, _, _) = counts_of(reg);
    let mut bad_act = OK;
    each(reg, |kind, b| {
        if kind == KIND_ACTION && (rd32(b) >= np || rd32(b.add(4)) >= image_size) {
            bad_act = E_FORMAT;
        }
    });
    bad_act
}

/// Native pages: each record, the claimed pages, the bindings against the machine's parameters.
pub(crate) unsafe fn ck_native<C: CoreApi>(_core: &C, c: &Ctx<'_>) -> i32 {
    let (reg, image_size, inf) = (c.reg, c.image_size, c.inf);
    let (np, _, _, _, _) = counts_of(reg);
    let mut native_set = PageSet::<MAX_MACH>::new();
    let mut bad_native = OK;
    each(reg, |kind, b| {
        if !record::is_kind(kind) || bad_native != OK {
            return;
        }
        bad_native = record::check(kind, b, np, image_size);
        if bad_native != OK {
            return;
        }
        let owner = rd32(b.add(4));
        if !native_set.claim(kind, owner, record::page_index(kind, b)) {
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
            let binding = b.add(record::binding_offset(kind) + 12 * i);
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
pub(crate) unsafe fn ck_device(c: &Ctx<'_>) -> i32 {
    let (reg, inf) = (c.reg, c.inf);
    let (_, _, ng, _, _) = counts_of(reg);
    // pages need a device that has page rows
    let has_pages = iw(inf, w::PG_ROWS) != 0;
    if ng != 0 && !has_pages {
        return E_FORMAT;
    }
    OK
}

/// The parameter, page and machine records, in file order.
pub(crate) unsafe fn ck_records<C: CoreApi>(core: &C, c: &Ctx<'_>) -> i32 {
    let (reg, inf) = (c.reg, c.inf);
    let (np, _, _, _, _) = counts_of(reg);
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
    each(reg, |kind, b| {
        if bad != OK || !(kind == KIND_PARAM || kind == KIND_PAGE || kind == KIND_MACHINE) {
            return; // an action and a native page were checked above, an effect belongs to another service: none has a machine, parameter or page row of this registry
        }
        if kind == KIND_PARAM {
            let (template, slot) = (rd32(b.add(4)), rd32(b.add(8)));
            // the index is its position, the names and the range: the rule of `ck_counts`; the template and the slot depend on the device
            if template >= p_first || slot < slot_base || slot >= slot_base + knobs {
                bad = E_FORMAT;
            }
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
            if knobs as usize != KNOBS {
                bad = E_FORMAT;
            }
        } else {
            // the first word is a machine key; `assign` has the number for it (0: none, which is out of range below)
            let t = crate::assign::resolve(core, rd32(b));
            let clone = rd32(b.add(4));
            let own = rd32(b.add(12));
            if own == 1 && !has_pages {
                bad = E_FORMAT;
                return;
            }
            let r = capacity::machine_type(t, dyn0, dyn_n, clone, m_first);
            if r != OK {
                bad = r;
                return;
            }
            // two keys that resolve to one type number (the keys differ: `ck_counts` has the same key twice)
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
            let name = core.rd(row + iw(inf, w::O_NAME));
            if mach_owner(core, k) != 0 || (name != 0 && core.rd8(name) != 0) {
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
        }
    });
    if bad != OK {
        return bad;
    }
    OK
}

/// A page belongs to a machine of the same file.
pub(crate) unsafe fn ck_owners<C: CoreApi>(core: &C, c: &Ctx<'_>) -> i32 {
    let reg = c.reg;
    let (_, _, _, _, _) = counts_of(reg);
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
pub(crate) unsafe fn ck_rows<C: CoreApi>(_core: &C, c: &Ctx<'_>) -> i32 {
    let (reg, inf) = (c.reg, c.inf);
    let (np, _, ng, _, _) = counts_of(reg);
    let s = c.state;
    // free reserved page and parameter rows
    return capacity::registry_rows(
        ng,
        capacity::free_rows(&s.page_owner, iw(inf, w::PG_DYN_N)),
        np,
        capacity::free_rows(&s.param_owner, iw(inf, w::P_DYN_N)),
    );
}

#[cfg(test)]
mod tests;
