//! Stage 1 over the real `StoreSource`: the real store on the host card model (`store_tests::Fixture`). The unit cases of the order of checks use a model
//! source (`StorageSource`); what only the store can show is that a read stops at the end of the file and that card errors reach the status.

use super::*;
use crate::host_io::card;
use crate::store_tests::{found, pattern, upload, Fixture, L};
use dkrt_sdk::OK;

/// A chunk buffer of the device's kind (16-byte aligned), filled with poison.
#[repr(C, align(16))]
struct Chunk([u8; 1024]);

const POISON: u8 = 0xa5;
const NAME: &str = "digikit/system.dksp";

fn put_package(body: &[u8]) -> Vec<u8> {
    let f = package(RESIDENT_TABLE_MAJOR, RESIDENT_TABLE_MINOR, body);
    assert_eq!(upload(NAME, &f), OK);
    f
}

fn load(chunk_len: usize) -> (PackageStatus, RecordingPlacer) {
    let mut chunk = Chunk([POISON; 1024]);
    let mut s = Stage1::new(StoreSource::new(PACKAGE_PATH), RecordingPlacer::default(), &mut chunk.0[..chunk_len]);
    let status = unsafe { s.load() };
    (status, s.placer)
}

#[test]
fn a_read_stops_at_the_end_of_a_small_file_and_does_not_touch_the_next_one() {
    let _f = Fixture::new();
    let f = put_package(&body(100));
    assert_eq!(upload("digikit/zz", &pattern(3000, 5)), OK);
    let mut chunk = Chunk([POISON; 1024]);
    let mut src = StoreSource::new(PACKAGE_PATH);
    unsafe {
        assert_eq!(src.open(), Ok(f.len() as u32));
        let reads = card(|c| c.reads);
        assert_eq!(src.read(0, &mut chunk.0), Ok(f.len()));
        assert_eq!(card(|c| c.reads), reads + 1);
        assert_eq!(&chunk.0[..f.len()], &f[..]);
        // one sector of the file was read, none of the file after it
        assert!(chunk.0[512..].iter().all(|b| *b == POISON));
        // nothing is read at or after the end
        assert_eq!(src.read(512, &mut chunk.0), Ok(0));
        assert_eq!(card(|c| c.reads), reads + 1);
    }
}

#[test]
fn the_package_loads_whole_as_the_last_file_of_the_store_and_before_another() {
    for after in [false, true] {
        let _f = Fixture::new();
        let b = body(1300);
        put_package(&b);
        if after {
            assert_eq!(upload("digikit/zz", &pattern(5000, 3)), OK);
        }
        for chunk in [512, 1024] {
            let (status, placer) = load(chunk);
            assert_eq!((status, placer.body), (PackageStatus::Ok, b.clone()), "chunk {chunk}, file after: {after}");
        }
    }
}

#[test]
fn a_card_that_cannot_be_read_in_the_body_is_unreadable_and_not_damaged() {
    let _f = Fixture::new();
    put_package(&body(1300));
    let file = found(NAME);
    card(|c| c.bad_reads.push(L.first + file.first + 2));
    let (status, placer) = load(512);
    assert_eq!(status, PackageStatus::Unreadable);
    assert!(placer.abandoned);
}

#[test]
fn a_card_without_the_file_is_missing() {
    let _f = Fixture::new();
    assert_eq!(load(512).0, PackageStatus::Missing, "no store");
    assert_eq!(upload("digikit/zz", &pattern(100, 5)), OK);
    assert_eq!(load(512).0, PackageStatus::Missing, "a store without the package");
}
