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

// ---- the package placer: the real artifact in the package block -------------------------------------------------------------------

mod package_block {
    use super::*;
    use crate::arena;
    use crate::fake_platform::test_arena::TestArena;
    use crate::platform::st;
    use dkrt_sdk::syspkg::body::BodyHeader;
    use dkrt_sdk::PACKAGE_BYTES;

    /// The arena after `init`, nothing allocated: all its bytes free, one block.
    fn whole_arena() -> (u32, u32) {
        (ARENA_BYTES as u32, ARENA_BYTES as u32 - dkneeds::ARENA_HDR)
    }

    const BASE_CHUNK: usize = 4096;

    /// Stage 1 over `file` with the device's placer, in a fresh arena; `then` sees the result while the arena is up.
    fn placed<R>(file: &[u8], then: impl FnOnce(PackageStatus, &Stage1<'_, StorageSource<'_>, PackagePlacer>) -> R) -> R {
        let _arena = TestArena::lock();
        let card = card_with(file);
        let mut chunk = vec![0u8; BASE_CHUNK];
        unsafe {
            arena::init();
            let mut s = Stage1::new(StorageSource::new(&card), PackagePlacer::new(), &mut chunk);
            let status = s.load();
            then(status, &s)
        }
    }

    /// A package file whose body header names `image` bytes of image, `bss` bytes of bss and the relocations `relocs` (image offsets).
    fn small_package(image: u32, bss: u32, relocs: &[u32]) -> Vec<u8> {
        let h = BodyHeader { entry: 2, image, bss, relocs: relocs.len() as u32, align: 4 };
        let mut b = h.encode().to_vec();
        b.extend((0..image).map(|i| (i * 5 + 1) as u8));
        for r in relocs {
            b.extend_from_slice(&r.to_be_bytes());
        }
        package(RESIDENT_TABLE_MAJOR, RESIDENT_TABLE_MINOR, &b)
    }

    fn round16(n: u32) -> u32 {
        (n + 15) & !15
    }

    #[test]
    fn the_built_artifact_sits_in_the_package_block_relocated_with_its_bss_cleared_and_the_arena_stays_whole() {
        let f = artifact();
        let body = &f[HEADER_BYTES..];
        let h = BodyHeader::decode(body).unwrap();
        let _arena = TestArena::lock();
        let card = card_with(&f);
        let mut chunk = vec![0u8; BASE_CHUNK];
        unsafe {
            arena::init();
            assert_eq!(arena::stats(), whole_arena());
            // dirty memory where the package will lie: a bss that stage 1 did not clear, or a relocation table left in it, shows
            let need = h.placed_bytes() as u32;
            let (block, bytes) = crate::state::package_block();
            core::ptr::write_bytes(block.ptr::<u8>(), 0xa5, round16(need) as usize);
            let mut s = Stage1::new(StorageSource::new(&card), PackagePlacer::new(), &mut chunk);
            assert_eq!(s.load(), PackageStatus::Ok);
            // the capacity report's `used` for PACKAGE: the header-derived placed size that stage 1 recorded, (the package's capacity report reads this word; its own test is in `package/capacity.rs`)
            assert_eq!(crate::platform::st().package_bytes, need);
            let (entry, minor) = s.placed().expect("a placed package has its entry");
            assert_eq!(minor, RESIDENT_TABLE_MINOR);
            let base = entry.get() - h.entry;
            // the package starts at its own block, which is `PACKAGE_BYTES` and holds it; the arena is as free as before: all 512 KiB
            assert_eq!(base - dkrt_sdk::syspkg::body::HEADER_BYTES as u32, block.get(), "the package starts at the package block");
            assert!(need <= bytes && bytes as usize == PACKAGE_BYTES);
            assert_eq!(arena::stats(), whole_arena(), "the package takes nothing of the arena");
            assert_eq!(arena::stats().0, ARENA_BYTES as u32);
            // the image is the unrelocated one plus the base at each relocation entry
            let mut want = body[dkrt_sdk::syspkg::body::HEADER_BYTES..][..h.image as usize].to_vec();
            relocate(&mut want, h.reloc_table(body), base).unwrap();
            let got = core::slice::from_raw_parts(DevAddr::new(base).ptr::<u8>(), h.image as usize);
            assert!(got == &want[..], "the placed image differs from the relocated one");
            // the bss after it is zero
            let bss = core::slice::from_raw_parts(DevAddr::new(base + h.image).ptr::<u8>(), h.bss as usize);
            assert!(bss.iter().all(|b| *b == 0), "the bss is cleared");
            // the entry is inside the image, and the table it was built against fits this core's
            assert!(entry.get() >= base && entry.get() < base + h.image);
            accept_table(&crate::resident::TABLE, minor).expect("the artifact's table fits this core");
            // an abandoned package leaves no placed package and the arena whole
            s.abandon();
            assert!(s.placed().is_none());
            assert_eq!(crate::platform::st().package_bytes, 0);
            assert_eq!(arena::stats(), whole_arena());
        }
    }

    #[test]
    fn a_package_larger_than_the_package_block_is_damaged_and_a_full_block_is_accepted() {
        // placed bytes: 32 header + image + the larger of bss and relocation table
        let fits = PACKAGE_BYTES as u32 - 32 - 16;
        placed(&small_package(16, fits, &[]), |status, s| {
            assert_eq!(status, PackageStatus::Ok, "a package that fills the block exactly is placed");
            assert!(s.placed().is_some());
            assert_eq!(unsafe { arena::stats() }, whole_arena());
        });
        placed(&small_package(16, fits + 1, &[]), |status, s| {
            assert_eq!(status, PackageStatus::Damaged, "one byte more is refused: the core-only cause of an unusable package");
            assert!(s.placed().is_none());
            assert_eq!(unsafe { arena::stats() }, whole_arena(), "the arena is untouched");
        });
    }

    #[test]
    fn a_body_that_has_no_body_header_is_damaged_before_anything_is_placed() {
        placed(&package(RESIDENT_TABLE_MAJOR, RESIDENT_TABLE_MINOR, &body(300)), |status, s| {
            assert_eq!(status, PackageStatus::Damaged);
            assert!(s.placed().is_none());
            assert!(unsafe { !arena::alloc(ARENA_BYTES as u32 - 64).is_null() });
        });
    }

    #[test]
    fn a_relocation_outside_the_image_is_damaged_and_the_block_goes_back() {
        // a relocation at 14 would be a word that ends past the 16-byte image
        placed(&small_package(16, 8, &[14]), |status, s| {
            assert_eq!(status, PackageStatus::Damaged);
            assert!(s.placed().is_none());
            assert!(unsafe { !arena::alloc(ARENA_BYTES as u32 - 64).is_null() });
        });
    }

    #[test]
    fn a_small_package_is_placed_whole_with_its_relocation_applied() {
        placed(&small_package(16, 8, &[4]), |status, s| {
            assert_eq!(status, PackageStatus::Ok);
            let (entry, _) = s.placed().unwrap();
            let base = entry.get() - 2;
            let image = unsafe { core::slice::from_raw_parts(DevAddr::new(base).ptr::<u8>(), 16) };
            let pattern: Vec<u8> = (0..16u32).map(|i| (i * 5 + 1) as u8).collect();
            // every byte is the file's, but the word at 4, which lies in the relocation table and has the base added
            assert_eq!(&image[..4], &pattern[..4]);
            assert_eq!(&image[8..], &pattern[8..]);
            let was = u32::from_be_bytes(pattern[4..8].try_into().unwrap());
            assert_eq!(u32::from_be_bytes(image[4..8].try_into().unwrap()), was.wrapping_add(base));
        });
    }
}
