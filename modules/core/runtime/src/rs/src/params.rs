//! Parameter table rows of the run-time registry: the kind-free writers that machines, effect knobs and send knobs share
//! (docs/design/digikit-registry.md). They read the row layout from `dynreg_info` through `registry::info`.
use crate::cstr_len;
use crate::registry::{info, iw, rd, w, wr, MACH_TEXT};

/// Write one 60-byte parameter table row from its lowering spec (the template's words, the record's values).
pub(crate) unsafe fn write_param_row(
    inf: *const u32,
    code: u32,
    template: u32,
    slot: u32,
    min: u32,
    max: u32,
    def: u32,
    name: u32,
    short: u32,
) {
    let table = iw(inf, w::P_TABLE);
    let size = iw(inf, w::PF_SIZE);
    let row = table + code * size;
    let tpl = table + template * size;
    // values by spec code 0x100 ..: min, max, default, name, short, mirror index, kind word, 0x80 + slot, the slot itself
    // (no `match`: a jump table would put data into the code section)
    let t = ((slot + 1) >> 2) & 1;
    let u = 1i32.wrapping_sub((slot >> 2) as i32);
    let kv = (0x10u32 + slot).wrapping_add((t as i32).wrapping_mul(u).wrapping_mul(0xffec) as u32);
    let vals: [u32; 16] = [
        min,
        max,
        def,
        name,
        short,
        25 + slot + (slot >> 2) * 2,
        (kv << 16) | (rd(tpl + 0x18) & 0xffff),
        0x80 + slot,
        slot,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ];
    let mut i = 0;
    while i < 15 {
        let spec = iw(inf, w::SPEC + i);
        let v = if spec <= 0x3c {
            rd(tpl + spec)
        } else {
            vals[(spec as usize - 0x100) & 15]
        };
        wr(row + 4 * i as u32, v);
        i += 1;
    }
    // the registry row (core/params): name, short, template, slot, min, max, default; pf_t2 reads `template`
    let reg_row = iw(inf, w::P_ROWS) + (code - iw(inf, w::P_FIRST)) * iw(inf, w::P_ROW_SIZE);
    wr(reg_row, name);
    wr(reg_row + 4, short);
    wr(reg_row + iw(inf, w::P_TEMPLATE_OFF), template);
    wr(reg_row + iw(inf, w::P_TEMPLATE_OFF) + 4, slot);
    wr(reg_row + iw(inf, w::P_TEMPLATE_OFF) + 8, min);
    wr(reg_row + iw(inf, w::P_TEMPLATE_OFF) + 12, max);
    wr(reg_row + iw(inf, w::P_TEMPLATE_OFF) + 16, def);
}

/// A parameter row for an effect knob (`core/fxpages`): the lowering of `write_param_row` with the kit's effect value index as the mirror index and the template's
/// own kind and key words (the reserved machine rows' `0x10 + slot` ids would collide with stock effect parameters).
pub(crate) unsafe fn write_fx_param_row(code: u32, template: u32, mirror: u32, min: u32, max: u32, def: u32, name: u32, short: u32) {
    let inf = info();
    if inf.is_null() {
        return;
    }
    write_param_row(inf, code, template, 0, min, max, def, name, short);
    let table = iw(inf, w::P_TABLE);
    let size = iw(inf, w::PF_SIZE);
    let (row, tpl) = (table + code * size, table + template * size);
    wr(row + 4, mirror);
    wr(row + 0x18, rd(tpl + 0x18));
    wr(row + 0x1c, rd(tpl + 0x1c));
}

/// A parameter row for a send knob (`core/fxpages`, finding 64): the lowering of `write_fx_param_row` with the value class of `class` (the display, and no p-lock: the
/// value class of the kit's Delay rows) and the owner of the stock send row `owner_of` (so the firmware's own set and get reach the current track's Sound array at `mirror`),
/// and no CC and no NRPN (the row's word 6 is -1: `ParameterSet::vfunc_25` sends nothing).
pub(crate) unsafe fn write_send_param_row(code: u32, class: u32, owner_of: u32, mirror: u32, name: u32, short: u32) {
    let inf = info();
    if inf.is_null() {
        return;
    }
    write_fx_param_row(code, class, mirror, 0, 0x7f00, 0, name, short);
    let table = iw(inf, w::P_TABLE);
    let size = iw(inf, w::PF_SIZE);
    let row = table + code * size;
    wr(row, rd(table + owner_of * size));
    wr(row + 0x18, 0xffff_ffff);
}

/// A parameter row back to the reserved state (an empty gap row, as `remove` leaves the machine parameters).
pub(crate) unsafe fn reset_param_row(code: u32) {
    let inf = info();
    if inf.is_null() {
        return;
    }
    let empty = iw(inf, w::EMPTY);
    write_param_row(inf, code, iw(inf, w::RESERVE_TEMPLATE), iw(inf, w::SLOT_BASE), 0, 0, 0, empty, empty);
}

/// Build a std::string representation {len, cap, refcount -1, chars, 0} at `rep` (upper-cased when `upper`); returns the
/// chars' address.
pub(crate) unsafe fn make_rep(rep: *mut u8, src: *const u8, upper: bool) -> u32 {
    let n = cstr_len(src, MACH_TEXT) as u32;
    crate::wr32(rep, n);
    crate::wr32(rep.add(4), n);
    crate::wr32(rep.add(8), 0xffff_ffff);
    let mut i = 0;
    while i < n as usize {
        let c = *src.add(i);
        *rep.add(12 + i) = if upper && c.is_ascii_lowercase() { c - 32 } else { c };
        i += 1;
    }
    *rep.add(12 + n as usize) = 0;
    rep as u32 + 12
}
