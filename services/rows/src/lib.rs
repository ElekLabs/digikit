//! Parameter table rows of the run-time registry: the kind-free writers that machines, effect knobs and send knobs share
//! (docs/design/digikit-registry.md). They read the row layout from `rows_info` (`inf`, the words of `rows_info::w`).
//!
//! The crate is compiled into `machines.dks` and `fx.dks`; neither service needs the other. Each writes only the rows its adapter reserves: the machines service
//! the rows `core/rows` reserves (`P_DYN0` for `P_DYN_N` rows of `rows_info`), the fx service the rows `core/fxpages` reserves (`P_FIRST` for 9 per effect row and
//! the send rows from `P_SEND` of `fxpages_info`). The two blocks of the one parameter table are disjoint (an assembly test reads both tables of the image).
#![no_std]
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::mem::{cstr_len, iw, wr32, FwMem};
pub mod builder;
pub mod rows_info;
use rows_info::{w, MACH_TEXT};

/// `ROWS`, the first word of `rows_info`, and the version of the layout this crate reads (2: the machine and page rows; 1 had the parameter rows only).
pub const ROWS_MAGIC: u32 = 0x524f_5753;
pub const ROWS_VERSION: u32 = 2;

/// The words of `rows_info` (`rows_info::w`), the table `core/rows` exports to the image's info table (`CoreApi::info`); null when the image has no such
/// table or it is not one.
pub unsafe fn rows_info<C: CoreApi>(core: &C) -> *const u32 {
    let t = core.info(dkneeds::keys::name_hash("rows_info"));
    if t.is_null() || iw(t, w::MAGIC) != ROWS_MAGIC || iw(t, w::VERSION) != ROWS_VERSION {
        return core::ptr::null();
    }
    t
}

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
    // the registry row (core/rows): name, short, template, slot, min, max, default; pf_t2 reads `template`
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

/// The address in the RAM copy of the parameter table (`P_TABLE` of `rows_info`) of the stock row at `row`, where `table` is the address of the stock table (both firmware
/// addresses, resolved by the port file). `None` for a null `inf` or table, a row that is not on a row boundary of the stock table, and a row at or above `P_FIRST`: only a
/// stock row has a stock address, the rows from `P_FIRST` on belong to the registry's writers.
unsafe fn stock_row(inf: *const u32, table: u32, row: u32) -> Option<u32> {
    if inf.is_null() || table == 0 || row < table {
        return None;
    }
    let size = iw(inf, w::PF_SIZE);
    let copy = iw(inf, w::P_TABLE);
    if size == 0
        || copy == 0
        || (row - table) % size != 0
        || (row - table) / size >= iw(inf, w::P_FIRST)
    {
        return None;
    }
    Some(copy + (row - table))
}

/// The minimum and the maximum of a stock row of the table's RAM copy (`None` for a row `stock_row` refuses).
pub unsafe fn stock_range(
    m: &impl FwMem,
    inf: *const u32,
    table: u32,
    row: u32,
) -> Option<(u32, u32)> {
    let at = stock_row(inf, table, row)?;
    Some((m.rd(at + 8), m.rd(at + 12)))
}

/// Set the minimum and the maximum of a stock row of the table's RAM copy (the words at +8 and +0xc of the row at the stock address `row`; `table` is the stock table's
/// address) and return the pair it held. `None`, nothing written, for a row `stock_row` refuses. The copy is the table every reader of the firmware uses after `core/rows`
/// repointed the stock references to it, so the new range is the one the firmware sees; the stock table in the image is not changed.
pub unsafe fn set_stock_range(
    m: &impl FwMem,
    inf: *const u32,
    table: u32,
    row: u32,
    min: u32,
    max: u32,
) -> Option<(u32, u32)> {
    let prev = stock_range(m, inf, table, row)?;
    let at = stock_row(inf, table, row)?;
    m.wr(at + 8, min);
    m.wr(at + 12, max);
    Some(prev)
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

    #[test]
    fn a_stock_row_of_the_copy_takes_a_range_and_gives_back_the_old_one() {
        use dkrt_test_support::FakeCore;
        // the stock table at 0x1000 (60-byte rows), its copy at 0x8000; registry rows from code 100 on
        let mut core = FakeCore::new();
        let mut t = std::vec![0u32; 90];
        t[w::P_TABLE] = 0x8000;
        t[w::PF_SIZE] = 60;
        t[w::P_FIRST] = 100;
        core.set_rows_info(t);
        let inf = unsafe { rows_info(&core) };
        let row = |code: u32| 0x1000 + 60 * code;
        unsafe {
            core.wr(0x8000 + 60 * 79 + 8, 0);
            core.wr(0x8000 + 60 * 79 + 12, 0x600);
            let prev = set_stock_range(&core, inf, 0x1000, row(79), 0, 0xe00);
            assert_eq!(prev, Some((0, 0x600)), "the pair that was there");
            assert_eq!(core.rd(0x8000 + 60 * 79 + 12), 0xe00, "the copy changed");
            assert_eq!(core.rd(row(79) + 12), 0, "the stock address is not written");
            // the pair goes back
            assert_eq!(
                set_stock_range(&core, inf, 0x1000, row(79), 0, 0x600),
                Some((0, 0xe00))
            );
            assert_eq!(core.rd(0x8000 + 60 * 79 + 12), 0x600);
        }
    }

    #[test]
    fn a_row_that_is_not_stock_and_a_missing_table_are_refused_without_a_write() {
        use dkrt_test_support::FakeCore;
        let mut core = FakeCore::new();
        let mut t = std::vec![0u32; 90];
        t[w::P_TABLE] = 0x8000;
        t[w::PF_SIZE] = 60;
        t[w::P_FIRST] = 100;
        core.set_rows_info(t);
        let inf = unsafe { rows_info(&core) };
        unsafe {
            // a registry row (code 100), a row off the 60-byte grid, below the table, no stock table
            for row in [0x1000 + 60 * 100, 0x1000 + 61, 0x0fff] {
                assert_eq!(
                    set_stock_range(&core, inf, 0x1000, row, 1, 2),
                    None,
                    "{row:#x}"
                );
            }
            assert_eq!(set_stock_range(&core, inf, 0, 0x1000, 1, 2), None);
            // the image has no registry table
            assert_eq!(
                set_stock_range(&core, core::ptr::null(), 0x1000, 0x1000, 1, 2),
                None
            );
            assert!(core.mem.borrow().is_empty(), "nothing was written");
        }
    }
}
