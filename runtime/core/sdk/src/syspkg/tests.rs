use super::*;

fn header() -> Header {
    let mut sha = [0u8; 32];
    for (i, b) in sha.iter_mut().enumerate() {
        *b = i as u8 + 1;
    }
    Header::new(2, 5, 167_000, sha)
}

#[test]
fn round_trip_and_trailing_body() {
    let h = header();
    let mut bytes = h.encode().to_vec();
    assert_eq!(bytes.len(), HEADER_BYTES);
    assert_eq!(Header::decode(&bytes), Ok(h));
    bytes.extend_from_slice(b"body follows");
    assert_eq!(Header::decode(&bytes), Ok(h));
}

#[test]
fn the_layout_is_fixed() {
    let b = header().encode();
    assert_eq!(&b[0..4], b"DKSP");
    assert_eq!(&b[4..8], &[0, 0, 0, 1]);
    assert_eq!(&b[8..12], &[0, 2, 0, 5]);
    assert_eq!(&b[12..16], &167_000u32.to_be_bytes());
    assert_eq!(b[16], 1);
    assert_eq!(b[47], 32);
}

#[test]
fn refuses_short_bad_magic_and_other_format() {
    let good = header().encode();
    assert_eq!(
        Header::decode(&good[..HEADER_BYTES - 1]),
        Err(HeaderError::Short)
    );
    assert_eq!(Header::decode(&[]), Err(HeaderError::Short));
    let mut bad = good;
    bad[0] ^= 1;
    assert_eq!(Header::decode(&bad), Err(HeaderError::BadMagic));
    let mut other = good;
    other[7] = 2;
    assert_eq!(Header::decode(&other), Err(HeaderError::BadFormat));
}

#[test]
fn newer_major_is_refused_older_major_too() {
    let h = header(); // built for table 2.5
    assert!(!h.compatible(1, 9));
    assert!(!h.compatible(3, 9));
}

#[test]
fn minor_not_newer_than_the_core_is_accepted() {
    let h = header();
    assert!(h.compatible(2, 5));
    assert!(h.compatible(2, 6));
    assert!(!h.compatible(2, 4));
}

#[test]
fn body_matches_needs_size_and_digest() {
    let h = header();
    let sha = *h.sha256();
    assert!(h.body_matches(167_000, &sha));
    assert!(!h.body_matches(166_999, &sha));
    let mut other = sha;
    other[31] ^= 0x80;
    assert!(!h.body_matches(167_000, &other));
}
