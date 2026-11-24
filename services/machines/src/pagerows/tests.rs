//! The state of the reserved page rows at the start of the service.
use super::*;
use dkrt_abi::mem::FwMem;
use dkrt_test_support::FakeCore;

const ROWS: u32 = 0x60_0000;
const SIZE: u32 = 0x54;

fn core(page_rows: u32, count: u32) -> FakeCore {
    let mut t = std::vec![0u32; 96];
    t[w::PG_ROWS] = page_rows;
    t[w::PG_ROW_SIZE] = SIZE;
    t[w::PG_FIRST] = 0;
    t[w::PG_DYN0] = 0;
    t[w::PG_DYN_N] = count;
    t[w::PGO_NAME] = 0;
    t[w::PGO_TITLE] = 4;
    t[w::PGO_C] = 8;
    t[w::PGO_S] = 0x30;
    t[w::PGO_MACH] = 0x2c;
    t[w::PG_FREE] = 0x7fff_ffff;
    let mut c = FakeCore::new();
    c.set_dynreg(t);
    c
}

#[test]
fn init_makes_every_reserved_page_row_a_free_row_with_a_string_name() {
    let c = core(ROWS, 3);
    // rows as the image has them before the first dispatch: the name and title are C strings of the image, a machine and codes are set
    for k in 0..3u32 {
        let row = ROWS + k * SIZE;
        c.mem.borrow_mut().insert(row, 0x4000_0000);
        c.mem.borrow_mut().insert(row + 4, 0x4000_0004);
        c.mem.borrow_mut().insert(row + 0x2c, 5);
        c.mem.borrow_mut().insert(row + 8, 0xd5);
    }
    unsafe { init(&c) };
    let rep = addr_of!(rstate().empty_rep) as u32 + 12;
    for k in 0..3u32 {
        let row = ROWS + k * SIZE;
        let word = |off: u32| unsafe { c.rd(row + off) };
        assert_eq!(
            (word(0), word(4)),
            (rep, rep),
            "row {k}: the empty string representation"
        );
        assert_eq!(word(0x2c), 0x7fff_ffff, "row {k}: no machine has it");
        assert_eq!(
            (word(8), word(8 + 4 * 8)),
            (0, 10),
            "row {k}: no knobs, the track level"
        );
    }
    // a string representation: length 0, capacity 0, reference count -1 (leaked), then the characters
    let h = rstate().empty_rep;
    assert_eq!((h[0], h[1], h[2]), (0, 0, 0xffff_ffff));
    assert!(
        c.calls().len() >= 3 * 12,
        "every word of every row is written"
    );
}

#[test]
fn a_device_without_page_rows_or_without_the_registry_is_left_alone() {
    let c = core(0, 8);
    unsafe { init(&c) };
    assert!(c.calls().is_empty());
    let none = FakeCore::new();
    unsafe { init(&none) };
    assert!(none.calls().is_empty());
}
