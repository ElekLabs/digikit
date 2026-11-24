//! Registration, descriptor and the encoder mapping.
use super::*;
use crate::native_page::fake_firmware::*;

#[test]
fn registration_rules() {
    let b = [[0u32; 3]; 8];
    let mut t = PageTable::new(0);
    let reg = |t: &mut PageTable, ty, idx, mode, flags, draw, words: &[u32]| {
        t.register(ty, idx, mode, flags, b"T", draw, 1, &b, words, 0, false)
    };
    assert!(
        reg(&mut t, TYPES as u32, 1, 1, 0, 1, &[]).is_err(),
        "type bound"
    );
    assert!(reg(&mut t, 0, 2, 1, 0, 1, &[]).is_err(), "index bound");
    assert!(reg(&mut t, 0, 1, 1, 0, 0, &[]).is_err(), "draw required");
    assert!(
        reg(&mut t, 0, 1, 1, FLAG_WIDE_CANVAS, 1, &[]).is_err(),
        "flags need v2"
    );
    assert!(
        reg(&mut t, 0, 1, 2, FLAG_STOCK_OVERLAY, 1, &[]).is_err(),
        "overlay is page 0"
    );
    assert!(
        reg(&mut t, 0, 0, 2, FLAG_WIDE_CANVAS, 1, &[]).is_err(),
        "wide is page 1"
    );
    assert!(reg(&mut t, 0, 1, 1, 0, 1, &[0; 9]).is_err(), "word bound");
    assert!(!t.armed());
    assert!(reg(&mut t, 0, 1, 1, 0, 1, &[]).is_ok());
    assert!(reg(&mut t, 0, 1, 1, 0, 1, &[]).is_err(), "twice");
    assert!(reg(&mut t, 0, 0, 2, 0, 1, &[]).is_err(), "modes do not mix");
    assert_eq!(t.page_count(0, 3), 4, "v1 appends");
    assert_eq!(t.page_count(5, 3), 3);
    t.remove(0);
    assert!(!t.armed() && t.page(0, 1).is_none());
    assert!(reg(&mut t, 0, 0, 2, 0, 1, &[]).is_ok());
    assert_eq!(t.page_count(0, 3), 2, "v2 replaces");
}

#[test]
fn registration_builds_name_and_descriptor() {
    let mut b = [[0u32; 3]; 8];
    b[2] = [1, 77, 2];
    b[3] = [2, 5, 3];
    let mut t = PageTable::new(0);
    t.register(
        1,
        1,
        1,
        0,
        b"A very long title that is cut at 23",
        1,
        1,
        &b,
        &[],
        0,
        false,
    )
    .unwrap();
    let p = t.page(1, 1).unwrap();
    assert_eq!(p.title(), b"A very long title that ");
    let d = p.descriptor();
    assert_eq!(d[2 + 2], 77);
    assert_eq!(d[10], 10);
    assert_eq!(d[5], 0, "a LOCAL control has no descriptor code");
    assert_eq!(d[0], p.name_ptr() as usize as u32);
    let mut bad = b;
    bad[0] = [1, 1, 8];
    assert!(t
        .register(2, 1, 1, 0, b"x", 1, 1, &bad, &[], 0, false)
        .is_err());
    assert!(
        t.page(2, 1).is_none() && t.page_count(2, 3) == 3,
        "a refusal writes nothing"
    );
}

#[test]
fn code_maps_encoders_for_native_pages_only() {
    let t = table(0, 1);
    let v = FakeView::new();
    assert_eq!(t.code(&v, 7), 40);
    assert_eq!(t.code(&v, 0), 0, "LOCAL is consumed");
    assert_eq!(t.code(&v, 4), 0, "unused is consumed");
    assert_eq!(t.code(&v, 8), -1);
    let mut other = FakeView::new();
    other.ty = 4;
    assert_eq!(t.code(&other, 7), -1);
    other.ty = -1;
    assert_eq!(t.code(&other, 7), -1);
    assert_eq!(PageTable::new(0).code(&v, 7), -1);
}

/// The firmware reads a name as a `std::string`: the descriptor's words 0 and 1 are the address of the characters, and length, capacity and a reference count of -1 sit in the
/// three words before them.
#[test]
fn descriptor_names_the_pages_own_string_in_its_table_slot() {
    // registered where it stays: the descriptor holds an address, so a table is not moved once it has pages
    let mut t = PageTable::new(0);
    let mut b = [[0u32; 3]; 8];
    b[1] = [1, 40, 7];
    t.register(3, 1, 2, 0, b"Title", 0x1000, 0x2000, &b, &[], 0, false)
        .unwrap();
    let p = t.page(3, 1).unwrap();
    let at = p.name_ptr() as usize as u32;
    let d = p.descriptor();
    assert_eq!((d[0], d[1]), (at, at), "long and short name");
    assert_eq!(p.descriptor_ptr() as usize % 4, 0);
    let rep = |k: usize| unsafe { (p.name_ptr().sub(12 - 4 * k) as *const u32).read_unaligned() };
    assert_eq!((rep(0), rep(1), rep(2) as i32), (5, 23, -1));
    assert_eq!(unsafe { *p.name_ptr().add(5) }, 0, "terminated");
    // the address is the slot's, not a copy's
    assert!(std::ptr::eq(
        t.page(3, 1).unwrap().descriptor_ptr(),
        p.descriptor_ptr()
    ));
}

/// The table holds the reserved run-time types only, numbered from the first one: a stock type or a type past the range has no pages.
#[test]
fn the_table_covers_the_reserved_types_from_the_first_one() {
    let b = [[0u32; 3]; 8];
    let mut t = PageTable::new(16);
    let reg = |t: &mut PageTable, ty| t.register(ty, 1, 1, 0, b"T", 1, 1, &b, &[], 0, false);
    assert!(reg(&mut t, 15).is_err(), "a stock type");
    assert!(
        reg(&mut t, 16 + TYPES as u32).is_err(),
        "past the reserved range"
    );
    assert!(reg(&mut t, 16).is_ok() && reg(&mut t, 16 + TYPES as u32 - 1).is_ok());
    assert!(t.page(16, 1).is_some() && t.page(15, 1).is_none() && t.page(0, 1).is_none());
    assert_eq!(t.page_count(16, 3), 4);
    assert_eq!(t.page_count(15, 3), 3, "a stock type keeps its pages");
    t.remove(15);
    t.remove(16);
    assert!(t.page(16, 1).is_none() && t.page(16 + TYPES as u32 - 1, 1).is_some());
}
