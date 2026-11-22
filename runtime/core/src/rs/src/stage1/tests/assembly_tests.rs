//! The packed fixture system package (`test/syspkg`, made by `patchkit syspkg pack`) through stage 1: one fit, one refusal.

use super::*;
use std::path::Path;

fn fixture() -> Vec<u8> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../dist/syspkg/test/system.dksp");
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}; run `pnpm nx:pilot run test/syspkg:pack`", p.display()))
}

#[test]
fn the_packed_system_package_loads_through_stage_1() {
    let f = fixture();
    let (status, placer) = run(&card_with(&f), 512);
    assert_eq!(status, PackageStatus::Ok);
    assert_eq!(placer.body, f[HEADER_BYTES..]);
    // the body is the committed fixture body, not an empty stub: stage 1 streamed more than one chunk
    assert!(f.len() > 512);
}

#[test]
fn a_corrupted_copy_of_the_packed_package_is_damaged() {
    let mut f = fixture();
    let last = f.len() - 1;
    f[last] ^= 0x01;
    let (status, placer) = run(&card_with(&f), 512);
    assert_eq!(status, PackageStatus::Damaged);
    assert!(placer.abandoned);
}
