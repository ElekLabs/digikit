use super::*;

fn path(s: &[u8]) -> [u8; 64] {
    let mut p = [0u8; 64];
    p[..s.len()].copy_from_slice(s);
    p
}

#[test]
fn a_file_name_matches_the_last_part_of_a_path_and_nothing_shorter() {
    let p = path(b"modules/machine/fadedlimes-analog.dkm");
    assert!(last_part_is(&p, b"fadedlimes-analog.dkm"));
    assert!(!last_part_is(&p, b"analog.dkm"), "the part after a dash is not a part of the path");
    assert!(!last_part_is(&p, b"machine"));
    assert!(!last_part_is(&p, b""));
    assert!(last_part_is(&path(b"a.dkm"), b"a.dkm"));
    assert!(!last_part_is(&path(b"a.dkm"), b"xa.dkm"));
}
