extern crate std;

use super::*;
use core::mem::{offset_of, size_of};
use std::boxed::Box;

static LIFE: u8 = 0;
static REC: u8 = 0;

fn table(t: &'static u8) -> *const () {
    t as *const u8 as *const ()
}

fn parts(list: &[PartRef]) -> &'static [PartRef] {
    Box::leak(list.to_vec().into_boxed_slice())
}

fn whole() -> Range<usize> {
    0..usize::MAX
}

fn lookup(row: &ProviderRow) -> CheckedRow<'_> {
    // SAFETY: the host test's parts are leaked boxes, readable for the whole test.
    unsafe { check(row, whole()) }.unwrap()
}

#[test]
fn a_part_is_found_with_its_table_and_version() {
    let row = ProviderRow::new(
        7,
        parts(&[
            PartRef::new(PART_LIFECYCLE, 1, table(&LIFE)),
            PartRef::new(PART_RECORDS, 2, table(&REC)),
        ]),
    );
    let c = lookup(&row);
    assert_eq!(c.name(), 7);
    let found = PartLookup::Found {
        table: table(&REC),
        version: 2,
    };
    assert_eq!(c.part(PART_RECORDS, 2), found);
    assert_eq!(c.part(PART_RECORDS, 5), found);
}

#[test]
fn a_part_the_service_did_not_adopt_is_missing() {
    let row = ProviderRow::new(7, parts(&[PartRef::new(PART_LIFECYCLE, 1, table(&LIFE))]));
    assert_eq!(lookup(&row).part(PART_TICK, 1), PartLookup::Missing);
}

#[test]
fn a_part_version_newer_than_the_core_knows_is_too_new() {
    let row = ProviderRow::new(
        7,
        parts(&[
            PartRef::new(PART_LIFECYCLE, 1, table(&LIFE)),
            PartRef::new(PART_QUERY, 3, table(&REC)),
        ]),
    );
    assert_eq!(
        lookup(&row).part(PART_QUERY, 2),
        PartLookup::TooNew { version: 3 }
    );
}

#[test]
fn a_duplicate_tag_a_missing_lifecycle_or_another_abi_is_refused() {
    let dup = ProviderRow::new(
        7,
        parts(&[
            PartRef::new(PART_LIFECYCLE, 1, table(&LIFE)),
            PartRef::new(PART_QUERY, 1, table(&LIFE)),
            PartRef::new(PART_QUERY, 2, table(&REC)),
        ]),
    );
    assert_eq!(unsafe { check(&dup, whole()) }.err(), Some(E_FORMAT));
    let none = ProviderRow::new(7, parts(&[PartRef::new(PART_QUERY, 1, table(&LIFE))]));
    assert_eq!(unsafe { check(&none, whole()) }.err(), Some(E_FORMAT));
    let mut row = ProviderRow::new(7, parts(&[PartRef::new(PART_LIFECYCLE, 1, table(&LIFE))]));
    row.abi = PROVIDER_ABI + 1;
    assert_eq!(unsafe { check(&row, whole()) }.err(), Some(E_FORMAT));
}

/// A row and its parts in one buffer, like a package.
#[repr(C)]
struct Pack {
    row: ProviderRow,
    parts: [PartRef; 1],
}

fn pack() -> &'static mut Pack {
    let p = Box::leak(Box::new(Pack {
        row: ProviderRow::new(7, &[]),
        parts: [PartRef::new(PART_LIFECYCLE, 1, table(&LIFE))],
    }));
    p.row.parts = p.parts.as_ptr();
    p.row.nparts = 1;
    p
}

fn extent(p: &Pack) -> Range<usize> {
    let a = p as *const Pack as usize;
    a..a + size_of::<Pack>()
}

#[test]
fn a_part_list_outside_the_package_or_null_or_too_long_is_refused() {
    let p = pack();
    assert!(unsafe { check(&p.row, extent(p)) }.is_ok());
    // The extent ends inside the part list, or starts after the row.
    let e = extent(p);
    assert_eq!(
        unsafe { check(&p.row, e.start..e.end - 1) }.err(),
        Some(E_FORMAT)
    );
    assert_eq!(
        unsafe { check(&p.row, e.start + 1..e.end) }.err(),
        Some(E_FORMAT)
    );
    let n = pack();
    n.row.nparts = 2;
    assert_eq!(unsafe { check(&n.row, extent(n)) }.err(), Some(E_FORMAT));
    let z = pack();
    z.row.parts = core::ptr::null();
    assert_eq!(unsafe { check(&z.row, whole()) }.err(), Some(E_FORMAT));
    let m = pack();
    m.row.parts = (m.row.parts as usize + 1) as *const PartRef;
    assert_eq!(unsafe { check(&m.row, extent(m)) }.err(), Some(E_FORMAT));
}

#[test]
#[should_panic]
fn more_parts_than_a_u16_are_refused() {
    static BIG: [PartRef; u16::MAX as usize + 1] =
        [PartRef::new(0, 0, core::ptr::null()); u16::MAX as usize + 1];
    ProviderRow::new(7, &BIG);
}

/// The 32-bit layout is asserted at compile time above; the host has 64-bit pointers, where only the order of the fields is the same.
#[test]
fn the_field_order_is_the_abi() {
    assert!(
        offset_of!(ProviderRow, abi) < offset_of!(ProviderRow, name)
            && offset_of!(ProviderRow, name) < offset_of!(ProviderRow, parts)
            && offset_of!(ProviderRow, parts) < offset_of!(ProviderRow, nparts)
            && offset_of!(ProviderRow, nparts) < offset_of!(ProviderRow, reserved)
    );
    assert!(
        offset_of!(PartRef, tag) < offset_of!(PartRef, version)
            && offset_of!(PartRef, version) < offset_of!(PartRef, reserved)
            && offset_of!(PartRef, reserved) < offset_of!(PartRef, table)
    );
}

#[test]
fn version_zero_is_refused() {
    let row = ProviderRow::new(7, parts(&[PartRef::new(PART_LIFECYCLE, 0, table(&LIFE))]));
    assert_eq!(
        lookup(&row).part(PART_LIFECYCLE, 1),
        PartLookup::TooNew { version: 0 }
    );
}
