//! Parameter table rows of the run-time registry: the kind-free writers that machines, effect knobs and send knobs share
//! (docs/design/digikit-registry.md). They read the row layout from `dynreg_info` (`inf`, the words of `dkrt_abi::registry::w`).
//!
//! The crate is compiled into `machines.dks` and `fx.dks`; neither service needs the other. Each writes only the rows its adapter reserves: the machines service
//! the rows `core/dynreg` reserves (`P_DYN0` for `P_DYN_N` rows of `dynreg_info`), the fx service the rows `core/fxpages` reserves (`P_FIRST` for 9 per effect row and
//! the send rows from `P_SEND` of `fxpages_info`). The two blocks of the one parameter table are disjoint (an assembly test reads both tables of the image).
#![no_std]
use dkrt_abi::mem::{cstr_len, iw, wr32, FwMem};
use dkrt_abi::registry::{w, MACH_TEXT};

/// Write one 60-byte parameter table row from its lowering spec (the template's words, the record's values).
pub unsafe fn write_param_row(
    m: &impl FwMem,
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
        (kv << 16) | (m.rd(tpl + 0x18) & 0xffff),
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
            m.rd(tpl + spec)
        } else {
            vals[(spec as usize - 0x100) & 15]
        };
        m.wr(row + 4 * i as u32, v);
        i += 1;
    }
    // the registry row (core/params): name, short, template, slot, min, max, default; pf_t2 reads `template`
    let reg_row = iw(inf, w::P_ROWS) + (code - iw(inf, w::P_FIRST)) * iw(inf, w::P_ROW_SIZE);
    m.wr(reg_row, name);
    m.wr(reg_row + 4, short);
    m.wr(reg_row + iw(inf, w::P_TEMPLATE_OFF), template);
    m.wr(reg_row + iw(inf, w::P_TEMPLATE_OFF) + 4, slot);
    m.wr(reg_row + iw(inf, w::P_TEMPLATE_OFF) + 8, min);
    m.wr(reg_row + iw(inf, w::P_TEMPLATE_OFF) + 12, max);
    m.wr(reg_row + iw(inf, w::P_TEMPLATE_OFF) + 16, def);
}

/// A parameter row for an effect knob (`core/fxpages`): the lowering of `write_param_row` with the kit's effect value index as the mirror index and the template's
/// own kind and key words (the reserved machine rows' `0x10 + slot` ids would collide with stock effect parameters).
pub unsafe fn write_fx_param_row(
    m: &impl FwMem,
    inf: *const u32,
    code: u32,
    template: u32,
    mirror: u32,
    min: u32,
    max: u32,
    def: u32,
    name: u32,
    short: u32,
) {
    if inf.is_null() {
        return;
    }
    write_param_row(m, inf, code, template, 0, min, max, def, name, short);
    let table = iw(inf, w::P_TABLE);
    let size = iw(inf, w::PF_SIZE);
    let (row, tpl) = (table + code * size, table + template * size);
    m.wr(row + 4, mirror);
    m.wr(row + 0x18, m.rd(tpl + 0x18));
    m.wr(row + 0x1c, m.rd(tpl + 0x1c));
}

/// A parameter row for a send knob (`core/fxpages`, finding 64): the lowering of `write_fx_param_row` with the value class of `class` (the display, and no p-lock: the
/// value class of the kit's Delay rows) and the owner of the stock send row `owner_of` (so the firmware's own set and get reach the current track's Sound array at `mirror`),
/// and no CC and no NRPN (the row's word 6 is -1: `ParameterSet::vfunc_25` sends nothing).
pub unsafe fn write_send_param_row(
    m: &impl FwMem,
    inf: *const u32,
    code: u32,
    class: u32,
    owner_of: u32,
    mirror: u32,
    name: u32,
    short: u32,
) {
    if inf.is_null() {
        return;
    }
    write_fx_param_row(m, inf, code, class, mirror, 0, 0x7f00, 0, name, short);
    let table = iw(inf, w::P_TABLE);
    let size = iw(inf, w::PF_SIZE);
    let row = table + code * size;
    m.wr(row, m.rd(table + owner_of * size));
    m.wr(row + 0x18, 0xffff_ffff);
}

/// A parameter row back to the reserved state (an empty gap row, as `remove` leaves the machine parameters).
pub unsafe fn reset_param_row(m: &impl FwMem, inf: *const u32, code: u32) {
    if inf.is_null() {
        return;
    }
    let empty = iw(inf, w::EMPTY);
    write_param_row(
        m,
        inf,
        code,
        iw(inf, w::RESERVE_TEMPLATE),
        iw(inf, w::SLOT_BASE),
        0,
        0,
        0,
        empty,
        empty,
    );
}

/// Build a std::string representation {len, cap, refcount -1, chars, 0} at `rep` (upper-cased when `upper`); returns the
/// chars' address.
pub unsafe fn make_rep(rep: *mut u8, src: *const u8, upper: bool) -> u32 {
    let n = cstr_len(src, MACH_TEXT) as u32;
    wr32(rep, n);
    wr32(rep.add(4), n);
    wr32(rep.add(8), 0xffff_ffff);
    let mut i = 0;
    while i < n as usize {
        let c = *src.add(i);
        *rep.add(12 + i) = if upper && c.is_ascii_lowercase() {
            c - 32
        } else {
            c
        };
        i += 1;
    }
    *rep.add(12 + n as usize) = 0;
    rep as u32 + 12
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_firmware_string_holds_length_capacity_refcount_and_the_characters() {
        let mut rep = [0xaau8; 64];
        let chars = unsafe { make_rep(rep.as_mut_ptr(), b"Name\0".as_ptr(), false) };
        assert_eq!(
            chars,
            rep.as_ptr() as usize as u32 + 12,
            "the address of the characters"
        );
        assert_eq!(rep[..12], [0, 0, 0, 4, 0, 0, 0, 4, 0xff, 0xff, 0xff, 0xff]);
        assert_eq!(&rep[12..17], b"Name\0");
        let mut up = [0u8; 64];
        unsafe { make_rep(up.as_mut_ptr(), b"Name 2\0".as_ptr(), true) };
        assert_eq!(&up[12..19], b"NAME 2\0");
    }
}
