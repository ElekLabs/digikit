//! `patchkit::syspkg::pack` writes what the runtime's header type reads back: the header fields, the body after it, and a digest that matches the body.

use dkrt_sdk::syspkg::{HEADER_BYTES, Header};
use patchkit::syspkg::pack;
use sha2::{Digest, Sha256};

#[test]
fn a_packed_file_reads_back_through_the_shared_header() {
    let body: Vec<u8> = (0..300).map(|i| i as u8).collect();
    let file = pack(&body, 2, 5).unwrap();
    let h = Header::decode(&file).unwrap();
    assert_eq!(
        (h.table_major(), h.table_minor(), h.size() as usize),
        (2, 5, body.len())
    );
    assert_eq!(&file[HEADER_BYTES..], &body[..]);
    let digest: [u8; 32] = Sha256::digest(&body).into();
    assert!(h.body_matches(body.len() as u32, &digest));
    // the table version it was built against decides who can load it
    assert!(h.compatible(2, 5) && h.compatible(2, 6) && !h.compatible(2, 4) && !h.compatible(3, 5));
}
