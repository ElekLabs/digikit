//! The record checks.
use super::*;

fn put(body: &mut [u8], offset: usize, value: u32) {
    body[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
}

#[test]
fn replacement_callback_checks_version_index_and_binding_bounds() {
    let mut body = [0u8; LEN_V2 as usize - 8];
    put(&mut body, 0, 2);
    put(&mut body, 8, 2);
    put(&mut body, 12, 4);
    body[16..20].copy_from_slice(b"PAGE");
    put(&mut body, 48, 1);
    put(&mut body, 56, 7);
    assert_eq!(unsafe { check(KIND_V2, body.as_ptr(), 1, 8) }, OK);
    put(&mut body, 44, 1);
    assert_eq!(unsafe { check(KIND_V2, body.as_ptr(), 1, 8) }, OK);
    let mut page1 = body;
    put(&mut page1, 40, 1);
    assert_eq!(unsafe { check(KIND_V2, page1.as_ptr(), 1, 8) }, E_FORMAT);
    put(&mut page1, 44, 2);
    assert_eq!(unsafe { check(KIND_V2, page1.as_ptr(), 1, 8) }, OK);
    put(&mut page1, 44, 3);
    assert_eq!(unsafe { check(KIND_V2, page1.as_ptr(), 1, 8) }, E_FORMAT);
    for (offset, value) in [(0, 1), (40, 2), (44, 2), (52, 1), (56, 8)] {
        let mut bad = body;
        put(&mut bad, offset, value);
        assert_eq!(unsafe { check(KIND_V2, bad.as_ptr(), 1, 8) }, E_FORMAT);
    }
}

#[test]
fn validates_callback_bounds_and_binding_records() {
    let mut body = [0u8; LEN as usize - 8];
    put(&mut body, 0, 1);
    put(&mut body, 8, 2);
    put(&mut body, 12, 4);
    body[16..20].copy_from_slice(b"PAGE");
    put(&mut body, 40, 1);
    assert_eq!(unsafe { check(KIND, body.as_ptr(), 1, 8) }, OK);
    for (offset, value) in [(0, 2), (8, 3), (12, 8), (40, 3), (44, 1), (48, 8)] {
        let mut invalid = body;
        put(&mut invalid, offset, value);
        assert_eq!(unsafe { check(KIND, invalid.as_ptr(), 1, 8) }, E_FORMAT);
    }
    let mut duplicate = body;
    put(&mut duplicate, 52, 2);
    assert_eq!(unsafe { check(KIND, duplicate.as_ptr(), 1, 8) }, E_FORMAT);
    let mut gap = body;
    put(&mut gap, 64, 2);
    put(&mut gap, 72, 2);
    assert_eq!(unsafe { check(KIND, gap.as_ptr(), 1, 8) }, E_FORMAT);
    let mut title = body;
    title[16..40].fill(b'A');
    assert_eq!(unsafe { check(KIND, title.as_ptr(), 1, 8) }, E_FORMAT);
}
