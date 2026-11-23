//! Registration, descriptor and the encoder mapping.
use super::*;
use crate::native_page::fake_firmware::*;

#[test]
fn registration_rules() {
    let b = [[0u32; 3]; 8];
    let mut t = PageTable::new();
    let reg = |t: &mut PageTable, ty, idx, mode, flags, draw, words: &[u32]| {
        t.register(ty, idx, mode, flags, b"T", draw, 1, &b, words, 0, false)
    };
    assert!(reg(&mut t, 32, 1, 1, 0, 1, &[]).is_err(), "type bound");
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
    let mut t = PageTable::new();
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
    assert_eq!(
        (d[5], d[0]),
        (0, 0),
        "a LOCAL control has no descriptor code"
    );
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
    assert_eq!(PageTable::new().code(&v, 7), -1);
}
