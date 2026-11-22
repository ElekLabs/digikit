//! The packed fixture system package (`test/syspkg`, made by `patchkit syspkg pack`) and the built package artifact (`core/runtime:package`, made by `patchkit syspkg
//! build`) through stage 1: one fit, one refusal for the fixture; the artifact's header, entry and relocations.

use super::*;
use dkrt_sdk::syspkg::body::{relocate, BodyHeader};
use std::path::Path;

fn artifact() -> Vec<u8> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../dist/syspkg/runtime/system.dksp");
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}; run `pnpm nx:pilot run core/runtime:package`", p.display()))
}

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

#[test]
fn the_built_package_artifact_loads_through_stage_1() {
    let f = artifact();
    let (status, placer) = run(&card_with(&f), 512);
    assert_eq!(status, PackageStatus::Ok);
    assert_eq!(placer.body, f[HEADER_BYTES..]);
}

/// The artifact is built against the table this core has, has an entry inside its image, and holds no address but its own: after it is relocated to a base, every
/// relocated word points into the package's memory (image and bss). An address of the firmware or of the resident core in the image would fall outside.
#[test]
fn the_built_package_artifact_has_its_entry_and_links_only_through_the_table() {
    let f = artifact();
    let header = Header::decode(&f).expect("the artifact has a header");
    assert_eq!((header.table_major(), header.table_minor()), (RESIDENT_TABLE_MAJOR, RESIDENT_TABLE_MINOR));
    let body = &f[HEADER_BYTES..];
    let h = BodyHeader::decode(body).expect("the body decodes");
    assert!(h.entry < h.image && h.relocs > 0);
    let table = h.reloc_table(body).to_vec();
    let mut image = body[dkrt_sdk::syspkg::body::HEADER_BYTES..][..h.image as usize].to_vec();
    let before = image.clone();
    const BASE: u32 = 0x4000_0000;
    relocate(&mut image, &table, BASE).expect("every relocation names a word of the image");
    let memory = h.memory() as u32;
    for r in table.chunks(4) {
        let at = u32::from_be_bytes([r[0], r[1], r[2], r[3]]) as usize;
        let was = u32::from_be_bytes([before[at], before[at + 1], before[at + 2], before[at + 3]]);
        let now = u32::from_be_bytes([image[at], image[at + 1], image[at + 2], image[at + 3]]);
        assert!(was <= memory, "the word at {at:#x} holds {was:#x}, outside the package's memory of {memory:#x} bytes");
        assert_eq!(now, was + BASE);
    }
}
