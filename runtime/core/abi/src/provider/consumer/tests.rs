extern crate std;
use super::*;
use std::vec::Vec;

#[test]
fn records_of_a_kind_in_file_order() {
    let recs: &[(u32, &[u8])] = &[(1, b"a"), (2, b"x"), (1, b"b")];
    let c = Consumer::new(ConsumerId::new(3), recs);
    assert_eq!(c.id().get(), 3);
    let got: Vec<&[u8]> = c.records(1).collect();
    assert_eq!(got, [&b"a"[..], &b"b"[..]]);
    assert_eq!(c.records(9).count(), 0);
}

/// A `register` section as the registry walker has it: 16 header bytes (magic, version, device, count), then (kind, whole length, body) records.
fn section(records: &[(u32, &[u8])]) -> Vec<u8> {
    let mut v = std::vec![0u8; 16];
    v[12..16].copy_from_slice(&(records.len() as u32).to_be_bytes());
    for (kind, body) in records {
        v.extend((*kind).to_be_bytes());
        v.extend(((body.len() + 8) as u32).to_be_bytes());
        v.extend(*body);
    }
    v
}

#[test]
fn a_section_gives_its_base_and_the_bodies_of_a_kind_in_file_order() {
    let reg = section(&[(7, b"one"), (8, b"mid"), (7, b"two!"), (9, b"")]);
    let c = unsafe { Consumer::of_section(ConsumerId::new(2), reg.as_ptr(), 0x4000) };
    assert_eq!((c.id().get(), c.base()), (2, 0x4000));
    let got: Vec<&[u8]> = c.records(7).collect();
    assert_eq!(got, [&b"one"[..], &b"two!"[..]]);
    assert_eq!(c.records(9).collect::<Vec<_>>(), [&b""[..]]);
    assert_eq!(c.records(1).count(), 0);
}

#[test]
fn a_consumer_without_a_section_has_no_records() {
    let c = unsafe { Consumer::of_section(ConsumerId::new(1), core::ptr::null(), 0) };
    assert_eq!(c.records(7).count(), 0);
}
