use super::sel::*;
use super::*;

fn example() -> Record {
    let mut r = Record::empty();
    r.seq = 7;
    r.boot_counter = 1;
    r.count = 2;
    r.paths[0][..13].copy_from_slice(b"modules/a.dkm");
    r.paths[1][..13].copy_from_slice(b"modules/b.dkm");
    r
}

/// The same record as `tools/patchkit/pk_dkrt_boot.py make_record(["modules/a.dkm", "modules/b.dkm"], seq=7, counter=1)`:
/// the layout of docs/design/digikit-store.md; the CRC is the one that Python's zlib computes.
#[test]
fn the_record_has_the_documented_bytes() {
    let mut b = [0u8; 512];
    encode(&example(), &mut b);
    assert_eq!(&b[0..4], b"DKSL");
    assert_eq!(
        &b[4..24],
        &[0, 0, 0, 1, 0, 0, 0, 7, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 2]
    );
    assert_eq!(&b[32..45], b"modules/a.dkm");
    assert_eq!(&b[32 + SEL_PATH..45 + SEL_PATH], b"modules/b.dkm");
    assert_eq!(&b[508..512], &0xce5a_5f11u32.to_be_bytes());
}

/// The sticky type map: entry 0 is bytes 28 to 31, entries 1 to 7 are bytes 480 to 507 (docs/design/digikit-store.md); the same bytes as
/// `make_record(..., tmap=...)` in tests/patchkit/test_patchkit_dkrt_boot.py. The words before and after keep their places.
#[test]
fn the_sticky_map_is_in_the_reserved_bytes() {
    let mut r = example();
    r.map = [0xe53b_3a10, 0xcf31_7d11, 0, 0, 0, 0, 0, 0x17];
    let mut b = [0u8; 512];
    encode(&r, &mut b);
    assert_eq!(&b[28..32], &0xe53b_3a10u32.to_be_bytes());
    assert_eq!(&b[480..484], &0xcf31_7d11u32.to_be_bytes());
    assert_eq!(&b[504..508], &0x17u32.to_be_bytes());
    assert_eq!(&b[24..28], &[0, 0, 0, 0]); // stable_blocks stays where it was
    assert_eq!(&b[32..45], b"modules/a.dkm");
    assert_eq!(decode(&b).unwrap().map, r.map);
    // a record without a map (the builder's, an older runtime's) has an empty one
    encode(&example(), &mut b);
    assert_eq!(decode(&b).unwrap().map, [0; MAP_N]);
}

#[test]
fn decode_takes_what_encode_made_and_refuses_damage() {
    let mut b = [0u8; 512];
    encode(&example(), &mut b);
    let r = decode(&b).unwrap();
    assert_eq!((r.seq, r.boot_counter, r.flags, r.count), (7, 1, 0, 2));
    assert_eq!(&r.paths[1][..13], b"modules/b.dkm");
    // a flipped bit anywhere before the CRC, a bad magic, a count over 8, a used path with no NUL
    for at in [0usize, 9, 40, 300, 507] {
        let mut c = b;
        c[at] ^= 1;
        assert!(decode(&c).is_none(), "byte {at}");
    }
    let mut c = [0u8; 512];
    assert!(decode(&c).is_none());
    let mut r2 = example();
    r2.count = 9;
    encode(&r2, &mut c);
    assert!(decode(&c).is_none());
    let mut r3 = example();
    r3.paths[0] = [b'x'; SEL_PATH];
    encode(&r3, &mut c);
    assert!(decode(&c).is_none());
}
