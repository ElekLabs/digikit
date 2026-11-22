//! Stage 1 over `FakeStorage` and a recording placer: the unit cases (logic of the order of checks and the streaming). The real packed package through it is
//! `assembly_tests`.

use super::load::BUSY_RETRIES;
use super::*;
use crate::fake_storage::FakeStorage;
use dkrt_abi::sha256::Sha;
use dkrt_abi::storage::{Storage, ROOT_STORE};
use dkrt_sdk::syspkg::{Header, HEADER_BYTES};
use dkrt_abi::dev_addr::DevAddr;
use dkrt_sdk::*;
use std::vec;
use std::vec::Vec;

mod assembly_tests;

const PATH: &[u8] = b"digikit/system.dksp";

/// The entry the recording placer gives for every body.
const ENTRY: u32 = 0x1000;

/// What a card does wrong, per source: the answers the next calls give instead of the file's bytes.
#[derive(Default)]
pub(crate) struct Faults {
    /// `open` answers this many times `E_BUSY` first.
    pub(crate) open_busy: u32,
    /// `open` fails with this.
    pub(crate) open_error: Option<i32>,
    /// A read at or after this offset fails with `E_READ`.
    pub(crate) read_error_from: Option<u32>,
    /// Reads answer this many times `E_BUSY` first.
    pub(crate) read_busy: u32,
}

/// A source over `FakeStorage`: the whole file is read once at `open`, then served as `PackageSource` says (never past the file; what is past it in
/// `dst` is poison, so a stage 1 that used it would fail its hash). The real `StoreSource` over a card is in `store_source_tests`.
pub(crate) struct StorageSource<'a> {
    pub(crate) storage: &'a FakeStorage,
    pub(crate) faults: Faults,
    bytes: Vec<u8>,
}

impl<'a> StorageSource<'a> {
    pub(crate) fn new(storage: &'a FakeStorage) -> Self {
        StorageSource { storage, faults: Faults::default(), bytes: Vec::new() }
    }
}

impl PackageSource for StorageSource<'_> {
    unsafe fn open(&mut self) -> Result<u32, i32> {
        if self.faults.open_busy > 0 {
            self.faults.open_busy -= 1;
            return Err(E_BUSY);
        }
        if let Some(e) = self.faults.open_error {
            return Err(e);
        }
        let st = self.storage.stat(ROOT_STORE, PATH)?;
        self.bytes = vec![0; st.size as usize];
        self.storage.read_all(ROOT_STORE, PATH, self.bytes.as_mut_ptr(), st.size)
    }

    unsafe fn read(&mut self, offset: u32, dst: &mut [u8]) -> Result<usize, i32> {
        if self.faults.read_busy > 0 {
            self.faults.read_busy -= 1;
            return Err(E_BUSY);
        }
        if self.faults.read_error_from.is_some_and(|from| offset >= from) {
            return Err(E_READ);
        }
        let at = (offset as usize).min(self.bytes.len());
        let n = (self.bytes.len() - at).min(dst.len());
        dst.fill(0xa5);
        dst[..n].copy_from_slice(&self.bytes[at..at + n]);
        Ok(n)
    }
}

/// Records what it was given.
#[derive(Default)]
pub(crate) struct RecordingPlacer {
    pub(crate) body: Vec<u8>,
    pub(crate) abandoned: bool,
    /// `begin` refuses when set.
    pub(crate) full: bool,
    /// `finish` refuses when set.
    pub(crate) unfinishable: bool,
    /// The first bytes of the body, as `begin` was given them.
    pub(crate) head: Vec<u8>,
    /// `finish` ran.
    pub(crate) finished: bool,
}

impl Placer for RecordingPlacer {
    fn begin(&mut self, size: u32, head: &[u8]) -> Result<(), i32> {
        if self.full {
            return Err(E_TOO_BIG);
        }
        self.head = head.to_vec();
        self.body = vec![0; size as usize];
        Ok(())
    }
    fn write(&mut self, offset: u32, bytes: &[u8]) {
        self.body[offset as usize..offset as usize + bytes.len()].copy_from_slice(bytes);
    }
    fn abandon(&mut self) {
        self.abandoned = true;
    }
    fn finish(&mut self) -> Result<DevAddr, i32> {
        self.finished = true;
        if self.unfinishable {
            return Err(E_FORMAT);
        }
        Ok(DevAddr::new(ENTRY))
    }
}

pub(crate) fn digest(body: &[u8]) -> [u8; 32] {
    let mut sha = Sha::new();
    let mut out = [0u8; 32];
    unsafe {
        sha.update(body.as_ptr(), body.len() as u32);
        sha.finish(out.as_mut_ptr());
    }
    out
}

/// A package file: the header (built against `major.minor`) and the body.
fn package(major: u16, minor: u16, body: &[u8]) -> Vec<u8> {
    let mut f = Header::new(major, minor, body.len() as u32, digest(body)).encode().to_vec();
    f.extend_from_slice(body);
    f
}

fn body(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 7 + 3) as u8).collect()
}

fn card_with(file: &[u8]) -> FakeStorage {
    let s = FakeStorage::new();
    s.add_file(ROOT_STORE, PATH, file, 0, [0; 32]);
    s
}

/// Load `storage`'s package through a chunk buffer of `chunk` bytes.
pub(crate) fn run(storage: &FakeStorage, chunk: usize) -> (PackageStatus, RecordingPlacer) {
    run_with(storage, chunk, Faults::default())
}

/// `run` over a card that does `faults`.
fn run_with(storage: &FakeStorage, chunk: usize, faults: Faults) -> (PackageStatus, RecordingPlacer) {
    let mut buf = vec![0u8; chunk];
    let mut source = StorageSource::new(storage);
    source.faults = faults;
    let mut s = Stage1::new(source, RecordingPlacer::default(), &mut buf);
    let status = unsafe { s.load() };
    (status, s.placer)
}

#[test]
fn a_good_package_is_placed_whole_across_several_chunks() {
    let b = body(1300);
    let (status, placer) = run(&card_with(&package(RESIDENT_TABLE_MAJOR, RESIDENT_TABLE_MINOR, &b)), 512);
    assert_eq!(status, PackageStatus::Ok);
    assert_eq!(placer.body, b);
    assert!(!placer.abandoned);
}

#[test]
fn chunk_edges_do_not_change_the_result() {
    // bodies that end on, just before and just after a chunk edge, and an empty body
    for n in [0, 1, 512 - HEADER_BYTES - 1, 512 - HEADER_BYTES, 512 - HEADER_BYTES + 1, 1024 - HEADER_BYTES, 2000] {
        let b = body(n);
        let (status, placer) = run(&card_with(&package(RESIDENT_TABLE_MAJOR, RESIDENT_TABLE_MINOR, &b)), 512);
        assert_eq!((status, placer.body), (PackageStatus::Ok, b), "body of {n} bytes");
    }
}

#[test]
fn a_changed_body_byte_is_damaged_and_abandoned() {
    let mut f = package(RESIDENT_TABLE_MAJOR, RESIDENT_TABLE_MINOR, &body(1300));
    f[HEADER_BYTES + 700] ^= 1;
    let (status, placer) = run(&card_with(&f), 512);
    assert_eq!(status, PackageStatus::Damaged);
    assert!(placer.abandoned && !placer.finished, "a body that does not match its hash is never made runnable");
}

#[test]
fn another_major_is_incompatible_and_places_nothing() {
    let (status, placer) = run(&card_with(&package(RESIDENT_TABLE_MAJOR + 1, 0, &body(100))), 512);
    assert_eq!(status, PackageStatus::Incompatible);
    assert!(placer.body.is_empty() && !placer.abandoned);
}

#[test]
fn a_package_newer_than_the_core_is_refused_an_older_or_equal_one_runs() {
    let b = body(100);
    let (newer, _) = run(&card_with(&package(RESIDENT_TABLE_MAJOR, RESIDENT_TABLE_MINOR + 1, &b)), 512);
    assert_eq!(newer, PackageStatus::Incompatible);
    // the core's table only grows by appending: a package that wants fewer functions runs
    for minor in 0..=RESIDENT_TABLE_MINOR {
        let (status, placer) = run(&card_with(&package(RESIDENT_TABLE_MAJOR, minor, &b)), 512);
        assert_eq!((status, placer.body), (PackageStatus::Ok, b.clone()), "minor {minor}");
    }
}

#[test]
fn a_file_of_another_magic_or_format_is_incompatible() {
    let mut f = package(RESIDENT_TABLE_MAJOR, RESIDENT_TABLE_MINOR, &body(100));
    f[0] ^= 0xff;
    assert_eq!(run(&card_with(&f), 512).0, PackageStatus::Incompatible);
    let mut f = package(RESIDENT_TABLE_MAJOR, RESIDENT_TABLE_MINOR, &body(100));
    f[7] = 9;
    assert_eq!(run(&card_with(&f), 512).0, PackageStatus::Incompatible);
}

#[test]
fn no_file_is_missing() {
    assert_eq!(run(&FakeStorage::new(), 512).0, PackageStatus::Missing);
}

#[test]
fn a_short_or_truncated_file_is_damaged() {
    let f = package(RESIDENT_TABLE_MAJOR, RESIDENT_TABLE_MINOR, &body(1300));
    for len in [0, HEADER_BYTES - 1, HEADER_BYTES, HEADER_BYTES + 600, f.len() - 1] {
        let (status, placer) = run(&card_with(&f[..len]), 512);
        assert_eq!(status, PackageStatus::Damaged, "{len} bytes");
        assert!(placer.body.is_empty(), "a truncated file is refused before anything is placed ({len} bytes)");
    }
}

#[test]
fn a_file_with_bytes_after_the_body_is_damaged() {
    let mut f = package(RESIDENT_TABLE_MAJOR, RESIDENT_TABLE_MINOR, &body(100));
    f.push(0);
    assert_eq!(run(&card_with(&f), 512).0, PackageStatus::Damaged);
}

#[test]
fn a_placer_that_cannot_hold_the_body_leaves_the_package_unusable() {
    let storage = card_with(&package(RESIDENT_TABLE_MAJOR, RESIDENT_TABLE_MINOR, &body(100)));
    let mut buf = vec![0u8; 512];
    let placer = RecordingPlacer { full: true, ..Default::default() };
    let mut s = Stage1::new(StorageSource::new(&storage), placer, &mut buf);
    assert_eq!(unsafe { s.load() }, PackageStatus::Damaged);
}

#[test]
fn a_package_that_cannot_be_made_runnable_is_damaged_and_abandoned() {
    let storage = card_with(&package(RESIDENT_TABLE_MAJOR, RESIDENT_TABLE_MINOR, &body(100)));
    let mut buf = vec![0u8; 512];
    let placer = RecordingPlacer { unfinishable: true, ..Default::default() };
    let mut s = Stage1::new(StorageSource::new(&storage), placer, &mut buf);
    assert_eq!(unsafe { s.load() }, PackageStatus::Damaged);
    assert!(s.placer().abandoned && s.placed().is_none());
}

#[test]
fn a_placed_package_gives_its_entry_and_the_minor_it_was_built_against_and_a_refused_one_gives_none() {
    let b = body(1300);
    let storage = card_with(&package(RESIDENT_TABLE_MAJOR, 0, &b));
    let mut buf = vec![0u8; 512];
    let mut s = Stage1::new(StorageSource::new(&storage), RecordingPlacer::default(), &mut buf);
    assert_eq!(unsafe { s.load() }, PackageStatus::Ok);
    assert_eq!(s.placed(), Some((DevAddr::new(ENTRY), 0)));
    // the placer saw the start of the body before the rest of it, and `finish` came after the hash matched
    assert_eq!(s.placer().head, b[..512 - HEADER_BYTES]);
    assert!(s.placer().finished);
    let storage = card_with(&package(RESIDENT_TABLE_MAJOR, RESIDENT_TABLE_MINOR + 1, &b));
    let mut s = Stage1::new(StorageSource::new(&storage), RecordingPlacer::default(), &mut buf);
    assert_eq!(unsafe { s.load() }, PackageStatus::Incompatible);
    assert!(s.placed().is_none() && !s.placer().finished);
}

#[test]
fn a_busy_store_is_asked_again_a_bounded_number_of_times() {
    let card = card_with(&package(RESIDENT_TABLE_MAJOR, RESIDENT_TABLE_MINOR, &body(1300)));
    let ok = Faults { open_busy: BUSY_RETRIES, read_busy: BUSY_RETRIES, ..Default::default() };
    assert_eq!(run_with(&card, 512, ok).0, PackageStatus::Ok);
    let stuck_open = Faults { open_busy: BUSY_RETRIES + 1, ..Default::default() };
    assert_eq!(run_with(&card, 512, stuck_open).0, PackageStatus::Unreadable);
    let stuck_read = Faults { read_busy: BUSY_RETRIES + 1, ..Default::default() };
    assert_eq!(run_with(&card, 512, stuck_read).0, PackageStatus::Unreadable);
}

#[test]
fn only_a_missing_file_is_missing_other_open_errors_are_unreadable() {
    let card = card_with(&package(RESIDENT_TABLE_MAJOR, RESIDENT_TABLE_MINOR, &body(100)));
    for e in [E_READ, E_BAD_STORE] {
        let faults = Faults { open_error: Some(e), ..Default::default() };
        assert_eq!(run_with(&card, 512, faults).0, PackageStatus::Unreadable, "error {e}");
    }
    let gone = Faults { open_error: Some(E_NOT_FOUND), ..Default::default() };
    assert_eq!(run_with(&card, 512, gone).0, PackageStatus::Missing);
}

#[test]
fn a_read_error_is_unreadable_at_the_header_and_in_the_middle_of_the_body() {
    let card = card_with(&package(RESIDENT_TABLE_MAJOR, RESIDENT_TABLE_MINOR, &body(2000)));
    for from in [0, 1024] {
        let faults = Faults { read_error_from: Some(from), ..Default::default() };
        let (status, placer) = run_with(&card, 512, faults);
        assert_eq!(status, PackageStatus::Unreadable, "from {from}");
        assert_eq!(placer.abandoned, from > 0, "a body that was begun is abandoned (from {from})");
    }
}

mod start_tests;
mod store_source_tests;
