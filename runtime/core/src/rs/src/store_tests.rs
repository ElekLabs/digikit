//! Host tests of the storage boundary that stays through the storage restructure (docs/plans/runtime-slimming.md, step S0): the real store, the real writer
//! and the real adapters over a model of the card (`host_io.rs`), driven only through what the rest of the system sees:
//!
//! - the SysEx store commands (`store_cmds`): their replies, as `DKRT_UPLOAD_LAST` and `RBODY` hold them;
//! - the SDK `file_*` calls (`store_writer::sdk_*`), run as the step of an I/O job;
//! - the `Storage` trait of `PLATFORM`, whose profile a test sets (`ProfileGuard`);
//! - the card gate (`card`) and the power-cut knobs of the writer.
//!
//! Nothing here names a buffer, a state machine or a helper of the current layout, so the tests hold when store, writer and routes move into other files.
//! The route table `dkrt_test_support::routes` is the one answer to "who serves this root"; `the_writer_routes_agree_with_the_route_table` here, the
//! adapter's check in `storage_adapter.rs` and the samples service's check in `services/samples/src/tracks.rs` each hold one decider to it.
//!
//! Not covered: the DT2 firmware file system's directory listing and the +Drive adapter's own behaviour (the device tests and the adapter's tests have them).
use crate::fake_platform::FakePlatform;
use crate::fake_storage::{answer, list as storage_list, ramp, rows};
use crate::hash32;
use crate::host_io::{card, card_flip, fs, pdw, CardGuard, FsGuard, PdwGuard};
use crate::platform::{rd32, wr32, ProfileGuard};
use crate::store::TEST_LAYOUT;
use crate::store_cmds::{self, CLEAR_MAGIC};
use crate::storage::route::root_route;
use crate::storage::sdk_files as sf;
use crate::store_writer::{self as sw};
use crate::test_lock::LOCK;
use crate::transport::{buffer, BODY_AT, DKRT_UPLOAD_LAST, RBODY};
use crate::{card as card_gate, platform::PLATFORM};
use core::ptr::{addr_of, addr_of_mut, null, read_volatile, write_volatile};
use dkrt_abi::platform::DeviceProfile;
use dkrt_abi::profiles;
use dkrt_abi::storage::*;
use dkrt_sdk::proto as p;
use dkrt_sdk::store::Layout;
use dkrt_sdk::{DkrtStoreEntry, CARD_UNKNOWN, OK};
use dkrt_test_support::routes::{Backend, Op, RouteRow, ROUTES};
use std::collections::BTreeMap;
use std::string::{String, ToString};
use std::sync::MutexGuard;
use std::vec;
use std::vec::Vec;

pub(crate) const L: Layout = Layout { first: 1000, sectors: 4096 };
/// The module that owns the SDK calls of these tests (a source index, as `io` gives it).
const OWNER: u8 = 3;

// ---- the fixture ----------------------------------------------------------------------------------------------------------------------

/// A blank card of the layout `L`, the card gate unchecked, the writer's statics at rest. Holds the test lock.
pub(crate) struct Fixture {
    _card: CardGuard,
    _lock: MutexGuard<'static, ()>,
}

impl Fixture {
    pub(crate) fn new() -> Fixture {
        let lock = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
        unsafe { rest() };
        *card_gate::info() = card_gate::CardInfo {
            state: CARD_UNKNOWN,
            first_nonblank: u32::MAX,
            hash: 0,
            refused: 0,
            refused_range: 0,
            read_errors: 0,
            checked: 0,
            claims: 0,
            writes: 0,
            sectors: 0,
        };
        TEST_LAYOUT.with(|c| c.set(Some(L)));
        crate::store::test_forget_checked();
        crate::platform::test_forget_staged_writer();
        Fixture { _card: CardGuard::install(), _lock: lock }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe { rest() };
    }
}

/// No session, no pending file, no lock, no knob.
unsafe fn rest() {
    sw::abort_sysex();
    sw::retire(OWNER);
    sw::pend_clear();
    sw::unlock();
    write_volatile(addr_of_mut!(sw::DKRT_UPLOAD_CUT), 0);
    write_volatile(addr_of_mut!(sw::DKRT_UPLOAD_WRITES), 0);
    write_volatile(addr_of_mut!(sw::DKRT_UPLOAD_TEAR), crate::safe_mode::Knob(0));
    sw::TEST_NOW = 0;
}

pub(crate) fn pattern(n: usize, m: usize) -> Vec<u8> {
    (0..n).map(|i| (i * m + 1) as u8).collect()
}

pub(crate) fn slot(data: &[u8]) -> [u8; 32] {
    unsafe { hash32::slot_of(data.as_ptr(), data.len() as u32) }
}

fn zpath(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

// ---- the SysEx store commands ---------------------------------------------------------------------------------------------------------

/// The status of the last reply.
fn status() -> i32 {
    unsafe { read_volatile(addr_of!(DKRT_UPLOAD_LAST)) }
}

/// The reply body after the status byte.
fn reply_body() -> *const u8 {
    unsafe { (addr_of!(RBODY) as *const u8).add(1) }
}

/// What the dispatcher does around a command that touches the store: the store lock.
fn locked<R>(f: impl FnOnce() -> R) -> R {
    assert!(sw::try_lock(), "the store lock is free");
    let r = f();
    sw::unlock();
    r
}

type Handler = unsafe fn(u32, *mut u8, *const u8, usize) -> i32;

fn send(h: Handler, body: &[u8]) -> i32 {
    locked(|| unsafe {
        h(1, buffer(), body.as_ptr(), body.len());
        status()
    })
}

fn mkdir(path: &str) -> i32 {
    send(store_cmds::cmd_mkdir, &zpath(path))
}

fn delete(path: &str) -> i32 {
    send(store_cmds::cmd_delete, &zpath(path))
}

pub(crate) fn rename(from: &str, to: &str) -> i32 {
    let mut b = zpath(from);
    b.extend(zpath(to));
    send(store_cmds::cmd_rename, &b)
}

fn clear_with(magic: u32) -> i32 {
    let mut b = [0u8; 4];
    unsafe { wr32(b.as_mut_ptr(), magic) };
    send(store_cmds::cmd_clear, &b)
}

pub(crate) fn begin(path: &str, size: usize, sha: [u8; 32]) -> i32 {
    let mut b = zpath(path);
    let at = b.len();
    b.extend([0u8; 4 + 32]);
    unsafe { wr32(b.as_mut_ptr().add(at), size as u32) };
    b[at + 4..].copy_from_slice(&sha);
    send(store_cmds::cmd_begin, &b)
}

fn data(off: usize, bytes: &[u8]) -> i32 {
    locked(|| unsafe {
        let b = buffer();
        wr32(b.add(BODY_AT), off as u32);
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), b.add(BODY_AT + 4), bytes.len());
        store_cmds::cmd_data(1, b, 4 + bytes.len());
        status()
    })
}

fn end() -> i32 {
    locked(|| unsafe {
        store_cmds::cmd_end(1, buffer());
        status()
    })
}

fn abort() -> i32 {
    unsafe {
        store_cmds::h_abort(1, buffer(), null(), 0);
    }
    status()
}

/// BEGIN, DATA in chunks of 4096, END: the first status that is not OK.
pub(crate) fn upload(path: &str, bytes: &[u8]) -> i32 {
    let r = begin(path, bytes.len(), slot(bytes));
    if r != OK {
        return r;
    }
    let mut off = 0;
    while off < bytes.len() {
        let n = (bytes.len() - off).min(4096);
        let r = data(off, &bytes[off..off + n]);
        if r != OK {
            abort();
            return r;
        }
        off += n;
    }
    end()
}

/// One LIST row: `(path, size, hash slot)`.
type Listed = (String, u32, [u8; 32]);

/// LIST with an optional prefix: the status, the count the reply names and the rows it carries.
fn list_cmd(prefix: Option<&str>) -> (i32, u32, Vec<Listed>) {
    let mut b = vec![0u8; 4];
    if let Some(p) = prefix {
        b.extend(zpath(p));
    }
    let r = send(store_cmds::cmd_list, &b);
    if r != OK {
        return (r, 0, Vec::new());
    }
    let mut out = Vec::new();
    unsafe {
        let rb = reply_body();
        let count = rd32(rb);
        let mut at = 5;
        for _ in 0..*rb.add(4) {
            let mut path = Vec::new();
            while *rb.add(at) != 0 {
                path.push(*rb.add(at));
                at += 1;
            }
            let size = rd32(rb.add(at + 1));
            let mut hash = [0u8; 32];
            core::ptr::copy_nonoverlapping(rb.add(at + 5), hash.as_mut_ptr(), 32);
            at += 5 + 32;
            out.push((String::from_utf8(path).unwrap(), size, hash));
        }
        (OK, count, out)
    }
}

/// Everything the store holds, by path.
fn held() -> BTreeMap<String, (u32, [u8; 32])> {
    let (r, count, rows) = list_cmd(None);
    assert_eq!(r, OK);
    assert_eq!(count as usize, rows.len(), "the reply carries every row");
    rows.into_iter().map(|(p, s, h)| (p, (s, h))).collect()
}

/// READ: the status and the bytes.
fn read_cmd(path: &str, off: u32, len: u32) -> (i32, Vec<u8>) {
    let mut b = zpath(path);
    let at = b.len();
    b.extend([0u8; 8]);
    unsafe {
        wr32(b.as_mut_ptr().add(at), off);
        wr32(b.as_mut_ptr().add(at + 4), len);
    }
    let r = send(store_cmds::cmd_read, &b);
    if r != OK {
        return (r, Vec::new());
    }
    unsafe {
        let rb = reply_body();
        let size = rd32(rb);
        let n = len.min(p::READ_MAX as u32).min(size - off) as usize;
        (OK, core::slice::from_raw_parts(rb.add(4), n).to_vec())
    }
}

/// VERIFY of the whole file or of `range` (offset, length): the status and the hash slot the card's bytes give.
fn verify_cmd(path: &str, range: Option<(u32, u32)>) -> (i32, [u8; 32]) {
    let mut b = zpath(path);
    if let Some((off, len)) = range {
        let at = b.len();
        b.extend([0u8; 8]);
        unsafe {
            wr32(b.as_mut_ptr().add(at), off);
            wr32(b.as_mut_ptr().add(at + 4), len);
        }
    }
    let r = send(store_cmds::cmd_verify, &b);
    let mut h = [0u8; 32];
    if r == OK {
        unsafe { core::ptr::copy_nonoverlapping(reply_body(), h.as_mut_ptr(), 32) };
    }
    (r, h)
}

/// The store file `path` as `crate::store::find` knows it: where it starts.
pub(crate) fn found(path: &str) -> crate::store::File {
    unsafe { crate::store::find(zpath(path).as_ptr()).unwrap_or_else(|e| panic!("{path}: {e}")) }
}

// ---- reads of a file in pieces ----------------------------------------------------------------------------------------------------------

#[test]
fn a_file_read_in_sectors_stops_at_the_end_of_the_file() {
    let _f = Fixture::new();
    let a = pattern(700, 7);
    assert_eq!(upload("a/x", &a), OK);
    assert_eq!(upload("a/y", &pattern(3000, 5)), OK);
    let file = found("a/x");
    let read = |from: u32, count: u32| {
        let mut buf = Aligned([0xee; 8192]);
        let r = unsafe { crate::store::read_file_sectors(&file, from, count, buf.0.as_mut_ptr(), buf.0.len()) };
        (r, buf)
    };
    // more sectors asked than the file has (a file followed by another one): two are read, and the bytes after them are untouched
    let (r, buf) = read(0, 8);
    assert_eq!(r, Ok(700));
    assert_eq!(&buf.0[..700], &a[..]);
    assert!(buf.0[1024..].iter().all(|b| *b == 0xee));
    // an offset near the end: the last sector alone, then nothing
    let (r, buf) = read(1, 4);
    assert_eq!(r, Ok(188));
    assert_eq!(&buf.0[..188], &a[512..]);
    assert!(buf.0[512..].iter().all(|b| *b == 0xee));
    let reads = card(|c| c.reads);
    for from in [2, 3, 1000] {
        assert_eq!(read(from, 4).0, Ok(0), "from {from}");
    }
    assert_eq!(card(|c| c.reads), reads, "no sector is read at or past the end");
    // a short count is what was asked
    assert_eq!(read(0, 1).0, Ok(512));
}

#[test]
fn the_last_file_of_the_store_reads_to_its_end_and_a_whole_sector_file_gives_its_size() {
    let _f = Fixture::new();
    assert_eq!(upload("a/x", &pattern(1024, 7)), OK);
    let file = found("a/x");
    let mut buf = Aligned([0xee; 8192]);
    let r = unsafe { crate::store::read_file_sectors(&file, 0, 16, buf.0.as_mut_ptr(), buf.0.len()) };
    assert_eq!(r, Ok(1024));
    assert!(buf.0[1024..].iter().all(|b| *b == 0xee));
}

// ---- the SDK file_* calls ---------------------------------------------------------------------------------------------------------------

#[repr(C, align(16))]
struct Aligned([u8; 8192]);

/// Run `f` as a step of an I/O job of `OWNER`: the context `file_*` calls need.
pub(crate) fn in_step<R>(f: impl FnOnce() -> R) -> R {
    unsafe { crate::io::test_in_step(OWNER, f) }
}

/// `file_create`: the handle or a status.
fn sdk_open(root: u32, path: &str, size: usize, flags: u32) -> i32 {
    let z = zpath(path);
    in_step(|| unsafe { sf::sdk_create(root, z.as_ptr(), size as u32, null(), flags) })
}

/// `file_create`, `file_write`, `file_close`: the first status that is not OK (the handle counts as OK).
pub(crate) fn sdk_put(root: u32, path: &str, bytes: &[u8], flags: u32) -> i32 {
    assert!(bytes.len() <= 8192);
    let mut buf = Aligned([0; 8192]);
    buf.0[..bytes.len()].copy_from_slice(bytes);
    let h = sdk_open(root, path, bytes.len(), flags);
    if h <= 0 {
        return h;
    }
    in_step(|| unsafe {
        let r = sf::sdk_write(h, 0, buf.0.as_ptr(), bytes.len() as u32);
        if r != OK {
            sf::sdk_abort(h);
            return r;
        }
        let mut out = [0u8; 4];
        sf::sdk_close(h, out.as_mut_ptr())
    })
}

fn sdk_delete(root: u32, path: &str) -> i32 {
    let z = zpath(path);
    in_step(|| unsafe { sf::sdk_delete(root, z.as_ptr()) })
}

const SAMPLES: u32 = dkrt_sdk::files::SAMPLES;
pub(crate) const STORE: u32 = dkrt_sdk::files::STORE;

// ---- write sessions -------------------------------------------------------------------------------------------------------------------

#[test]
fn a_write_session_leaves_a_file_that_lists_reads_and_verifies_with_its_hash() {
    let _f = Fixture::new();
    let bytes = pattern(10_000, 7);
    assert_eq!(begin("cases/a", bytes.len(), slot(&bytes)), OK);
    assert_eq!(data(0, &bytes[..4096]), OK);
    assert_eq!(data(4096, &bytes[4096..8192]), OK);
    assert_eq!(data(8192, &bytes[8192..]), OK, "a last chunk may be short");
    assert_eq!(end(), OK);
    let h = held();
    assert_eq!(h.len(), 1);
    assert_eq!(h["cases/a"], (10_000, slot(&bytes)), "the table's hash is the hash of the bytes sent");
    let (r, got) = read_cmd("cases/a", 9000, 500);
    assert_eq!((r, got.as_slice()), (OK, &bytes[9000..9500]));
    assert_eq!(verify_cmd("cases/a", None), (OK, slot(&bytes)));
    assert_eq!(verify_cmd("cases/a", Some((512, 1024))), (OK, slot(&bytes[512..1536])), "a range of the file");
    assert_eq!(verify_cmd("cases/a", Some((100, 512))).0, p::E_OFFSET, "an offset is a multiple of 512");
    assert_eq!(verify_cmd("cases/a", Some((512, 20_000))).0, p::E_LENGTH);
    assert_eq!(verify_cmd("cases/none", None).0, p::E_NOFILE);
    assert!(!sw::is_open());
}

#[test]
fn a_file_replaces_the_file_of_its_path_and_an_empty_file_is_a_file() {
    let _f = Fixture::new();
    assert_eq!(upload("cases/a", &pattern(1000, 3)), OK);
    assert_eq!(upload("cases/a", &pattern(600, 5)), OK);
    assert_eq!(upload("cases/empty", &[]), OK);
    let h = held();
    assert_eq!(h["cases/a"], (600, slot(&pattern(600, 5))));
    assert_eq!(h["cases/empty"].0, 0);
    assert_eq!(h.len(), 2);
}

#[test]
fn a_bad_hash_refuses_the_end_and_leaves_no_file() {
    let _f = Fixture::new();
    let bytes = pattern(2000, 7);
    assert_eq!(upload("cases/keep", &pattern(300, 9)), OK);
    assert_eq!(begin("cases/a", bytes.len(), slot(&pattern(2000, 11))), OK);
    assert_eq!(data(0, &bytes[..1536]), OK);
    assert_eq!(data(1536, &bytes[1536..]), OK);
    assert_eq!(end(), p::E_HASH);
    assert!(!sw::is_open(), "the session is closed whatever the result");
    assert_eq!(held().keys().collect::<Vec<_>>(), ["cases/keep"]);
    // an upload that stops short of its size is refused too
    assert_eq!(begin("cases/b", 1000, slot(&pattern(1000, 3))), OK);
    assert_eq!(data(0, &pattern(512, 3)), OK);
    assert_eq!(end(), p::E_LENGTH);
    assert_eq!(held().len(), 1);
}

#[test]
fn verify_names_the_bytes_on_the_card_not_the_table() {
    let _f = Fixture::new();
    let bytes = pattern(3000, 7);
    assert_eq!(upload("cases/a", &bytes), OK);
    let listed = held()["cases/a"].1;
    assert_eq!(verify_cmd("cases/a", None), (OK, listed), "good");
    card_flip(L.first + found("cases/a").first + 1, 10, 0x01);
    let (r, now) = verify_cmd("cases/a", None);
    assert_eq!(r, OK);
    assert_ne!(now, listed, "bad: a bit of the data sector flipped");
    assert_eq!(held()["cases/a"].1, listed, "the table still says what was written");
    // the boot's read of the file checks the table's hash: E_SHA, and VERIFY names the hash of the flipped bytes
    let mut flipped = bytes.clone();
    flipped[512 + 10] ^= 0x01;
    assert_eq!(now, slot(&flipped), "VERIFY hashes the flipped bytes");
    assert_eq!(unsafe { crate::store::load_to_stage(&found("cases/a")) }, dkrt_sdk::E_SHA, "a file that no longer matches its table hash is not loaded");
}

/// The file is read for a load when its bytes are as written.
#[test]
fn a_file_read_for_a_load_matches_its_table_hash() {
    let _f = Fixture::new();
    assert_eq!(upload("cases/a", &pattern(3000, 7)), OK);
    assert_eq!(unsafe { crate::store::load_to_stage(&found("cases/a")) }, OK);
}

#[test]
fn an_abort_leaves_nothing_and_a_later_end_has_no_session() {
    let _f = Fixture::new();
    let bytes = pattern(3000, 7);
    assert_eq!(upload("cases/keep", &pattern(300, 9)), OK);
    assert_eq!(begin("cases/a", bytes.len(), slot(&bytes)), OK);
    assert_eq!(data(0, &bytes[..2048]), OK);
    assert!(sw::is_open());
    abort();
    assert!(!sw::is_open());
    assert_eq!(end(), p::E_STATE);
    assert_eq!(data(2048, &bytes[2048..]), p::E_STATE);
    assert_eq!(held().keys().collect::<Vec<_>>(), ["cases/keep"]);
    // an SDK session aborts the same way
    let h = sdk_open(STORE, "cases/b", 1000, 0);
    assert!(h > 0 && sw::is_open());
    assert_eq!(in_step(|| unsafe { sf::sdk_abort(h) }), OK);
    assert!(!sw::is_open());
    assert_eq!(held().len(), 1);
}

#[test]
fn an_sdk_file_lists_and_replaces_only_with_the_replace_flag() {
    let _f = Fixture::new();
    let a = pattern(3000, 7);
    assert_eq!(sdk_put(STORE, "cases/a", &a, 0), OK);
    assert_eq!(held()["cases/a"], (3000, slot(&a)));
    assert_eq!(sdk_put(STORE, "cases/a", &pattern(10, 3), 0), dkrt_sdk::E_EXISTS, "a second create of the path is refused");
    assert_eq!(held()["cases/a"], (3000, slot(&a)), "and changes nothing");
    assert_eq!(sdk_put(STORE, "cases/a", &pattern(10, 3), sf::FILE_REPLACE), OK);
    assert_eq!(held()["cases/a"], (10, slot(&pattern(10, 3))));
    assert!(!sw::is_open());
}

// ---- pending files (DK_FILE_DEFER) ----------------------------------------------------------------------------------------------------

#[test]
fn deferred_files_are_pending_until_committed_and_then_listed_with_their_hashes() {
    let _f = Fixture::new();
    let (a, b) = (pattern(1500, 7), pattern(700, 3));
    assert_eq!(sdk_put(STORE, "p/a", &a, sf::FILE_DEFER), OK);
    assert_eq!(sdk_put(STORE, "p/b", &b, sf::FILE_DEFER), OK);
    assert_eq!(in_step(|| unsafe { sf::sdk_pending() }), 2, "both are verified and named in no table yet");
    assert_eq!(in_step(|| unsafe { sf::sdk_commit() }), OK);
    assert_eq!(in_step(|| unsafe { sf::sdk_pending() }), 0);
    let h = held();
    assert_eq!((h["p/a"], h["p/b"]), ((1500, slot(&a)), (700, slot(&b))));
    assert_eq!(verify_cmd("p/a", None), (OK, slot(&a)));
}

#[test]
fn a_sysex_table_change_names_the_pending_files_first() {
    let _f = Fixture::new();
    let a = pattern(1500, 7);
    assert_eq!(sdk_put(STORE, "p/a", &a, sf::FILE_DEFER), OK);
    assert_eq!(mkdir("other"), OK);
    let h = held();
    assert_eq!(h["p/a"], (1500, slot(&a)));
    assert!(h.contains_key("other"));
    assert_eq!(in_step(|| unsafe { sf::sdk_pending() }), 0);
}

#[test]
fn a_deleted_pending_path_is_gone_without_a_commit() {
    let _f = Fixture::new();
    assert_eq!(sdk_put(STORE, "p/a", &pattern(1500, 7), sf::FILE_DEFER), OK);
    assert_eq!(sdk_delete(STORE, "p/a"), OK);
    assert_eq!(in_step(|| unsafe { sf::sdk_pending() }), 0);
    assert_eq!(in_step(|| unsafe { sf::sdk_commit() }), OK);
    assert!(held().is_empty());
}

// ---- rename, mkdir, delete, clear -----------------------------------------------------------------------------------------------------

#[test]
fn rename_moves_an_entry_with_its_data_and_hash() {
    let _f = Fixture::new();
    let a = pattern(2500, 7);
    assert_eq!(upload("old/a", &a), OK);
    assert_eq!(upload("old/keep", &pattern(100, 3)), OK);
    assert_eq!(rename("old/a", "new/b"), OK);
    let h = held();
    assert_eq!(h["new/b"], (2500, slot(&a)));
    assert!(!h.contains_key("old/a"));
    assert_eq!(verify_cmd("new/b", None), (OK, slot(&a)), "the bytes did not move");
    assert_eq!(rename("old/missing", "new/c"), p::E_NOFILE);
    assert_eq!(rename("old/keep", "new/b"), p::E_PATH, "the target is taken");
    assert_eq!(rename("old/keep", "old/keep/inside"), p::E_PATH, "inside itself, or inside a file");
    assert_eq!(held().len(), 2);
}

#[test]
fn mkdir_makes_a_folder_entry_that_a_file_inside_replaces() {
    let _f = Fixture::new();
    assert_eq!(mkdir("d"), OK);
    assert_eq!(mkdir("d"), OK, "again is ok");
    assert_eq!(held()["d"], (0, [0; 32]), "a folder entry has size 0 and a zero hash slot");
    assert_eq!(upload("d/x", &pattern(100, 3)), OK);
    let h = held();
    assert!(h.contains_key("d/x") && !h.contains_key("d"), "a file inside makes the folder entry unnecessary");
    assert_eq!(mkdir("d/x"), p::E_PATH, "a file is not a folder");
    assert_eq!(mkdir("d/x/y"), p::E_PATH, "nor is a path inside one");
    assert_eq!(mkdir("d"), OK, "a folder with files in it exists");
    assert_eq!(held().len(), 1);
}

#[test]
fn delete_removes_a_file_or_a_folder_entry_and_frees_its_space() {
    let _f = Fixture::new();
    assert_eq!(upload("a/x", &pattern(3000, 7)), OK);
    assert_eq!(mkdir("empty"), OK);
    let first = found("a/x").first;
    assert_eq!(delete("a/x"), OK);
    assert_eq!(delete("empty"), OK);
    assert_eq!(delete("a/x"), p::E_NOFILE);
    assert!(!held().contains_key("a/x") && !held().contains_key("empty"));
    // the extent is free again: the same file fits at the same place
    assert_eq!(upload("a/y", &pattern(3000, 7)), OK);
    assert_eq!(found("a/y").first, first);
}

#[test]
fn clear_empties_the_store_only_with_its_magic() {
    let _f = Fixture::new();
    assert_eq!(upload("a/x", &pattern(3000, 7)), OK);
    assert_eq!(upload("b/y", &pattern(100, 7)), OK);
    assert_eq!(clear_with(CLEAR_MAGIC ^ 1), p::E_MSG);
    assert_eq!(held().len(), 2);
    assert_eq!(clear_with(CLEAR_MAGIC), OK);
    assert!(held().is_empty());
    assert_eq!(upload("c/z", &pattern(100, 7)), OK, "a cleared store takes files");
}

#[test]
fn a_file_a_reader_holds_is_not_deleted_replaced_or_cleared() {
    let _f = Fixture::new();
    let a = pattern(3000, 7);
    assert_eq!(upload("a/x", &a), OK);
    let file = found("a/x");
    let lease = locked(|| sw::lease_take(0, file.first, file.size));
    assert!(lease > 0);
    assert_eq!(delete("a/x"), p::E_FILE_IN_USE);
    assert_eq!(clear_with(CLEAR_MAGIC), p::E_FILE_IN_USE);
    assert_eq!(begin("a/x", 10, slot(&pattern(10, 3))), p::E_FILE_IN_USE, "a replacement would free the extent");
    assert_eq!(held()["a/x"], (3000, slot(&a)));
    assert_eq!(sw::lease_end(lease), OK);
    assert_eq!(delete("a/x"), OK);
}

// ---- lock and session refusals --------------------------------------------------------------------------------------------------------

#[test]
fn an_open_sdk_session_makes_every_table_change_busy() {
    let _f = Fixture::new();
    assert_eq!(upload("a/x", &pattern(100, 7)), OK);
    let h = sdk_open(STORE, "b/y", 1000, 0);
    assert!(h > 0);
    assert_eq!(begin("c/z", 10, slot(&pattern(10, 3))), dkrt_sdk::E_BUSY);
    assert_eq!(mkdir("d"), dkrt_sdk::E_BUSY);
    assert_eq!(rename("a/x", "a/y"), dkrt_sdk::E_BUSY);
    assert_eq!(delete("a/x"), dkrt_sdk::E_BUSY);
    assert_eq!(clear_with(CLEAR_MAGIC), dkrt_sdk::E_BUSY);
    assert_eq!(sdk_open(STORE, "e/w", 10, 0), dkrt_sdk::E_BUSY, "one session at a time");
    assert_eq!(held().keys().collect::<Vec<_>>(), ["a/x"], "reads are not refused, and nothing changed");
    assert_eq!(in_step(|| unsafe { sf::sdk_abort(h) }), OK);
    assert_eq!(mkdir("d"), OK, "free again");
}

#[test]
fn an_open_sysex_session_keeps_the_sdk_out_and_a_new_begin_takes_its_place() {
    let _f = Fixture::new();
    assert_eq!(begin("a/x", 1000, slot(&pattern(1000, 3))), OK);
    assert_eq!(sdk_open(STORE, "b/y", 10, 0), dkrt_sdk::E_BUSY);
    assert_eq!(begin("a/z", 600, slot(&pattern(600, 3))), OK, "a SysEx BEGIN aborts the one before");
    assert_eq!(data(0, &pattern(600, 3)), OK);
    assert_eq!(end(), OK);
    assert_eq!(held().keys().collect::<Vec<_>>(), ["a/z"]);
}

#[test]
fn a_held_store_lock_makes_an_sdk_call_busy_before_it_changes_anything() {
    let _f = Fixture::new();
    assert!(sw::try_lock());
    assert_eq!(sdk_open(STORE, "a/x", 100, 0), dkrt_sdk::E_BUSY);
    assert!(!sw::is_open());
    sw::unlock();
    assert!(sdk_open(STORE, "a/x", 100, 0) > 0);
}

#[test]
fn a_foreign_card_refuses_every_write_and_changes_nothing() {
    let _f = Fixture::new();
    // somebody else's data in the area: not blank, not ours
    card(|c| {
        c.sectors.insert(L.first + 5, [0x55; 512]);
    });
    unsafe { card_gate::check() };
    assert_eq!(card_gate::state(), dkrt_sdk::CARD_FOREIGN);
    let writes = card(|c| c.writes);
    assert_eq!(begin("a/x", 100, slot(&pattern(100, 3))), p::E_INUSE);
    assert_eq!(mkdir("d"), p::E_INUSE);
    assert_eq!(delete("a/x"), p::E_INUSE);
    assert_eq!(clear_with(CLEAR_MAGIC), p::E_INUSE);
    assert_eq!(sdk_open(STORE, "a/x", 100, 0), p::E_INUSE);
    assert_eq!(card(|c| c.writes), writes, "nothing reached the card");
}

/// A fresh card is ours after the selection record and has no store header until the first upload: INFO's view, the module list and the HEALTH
/// figures all see an empty store (finding 37); a foreign area has no store for any of them.
#[test]
fn an_ours_card_without_headers_is_an_empty_store_for_the_list_and_the_figures() {
    let _f = Fixture::new();
    card(|c| {
        c.sectors.insert(L.first + L.sectors - 1, [0u8; 512]);
    });
    unsafe {
        card_gate::check();
        crate::store::figures_stale();
        let mut list = [const { unsafe { core::mem::zeroed::<DkrtStoreEntry>() } }; 4];
        assert_eq!(crate::store::list(b"modules/\0".as_ptr(), list.as_mut_ptr(), 4), 0);
        let (used, free) = crate::store::seen().expect("an empty store has figures");
        assert_eq!(used, 0);
        assert_eq!(free, (L.limit() - 1 - dkrt_sdk::store::table_sectors(1)) / 2);
        let mut h: dkrt_sdk::DkrtHealth = core::mem::zeroed();
        crate::store::figures(&mut h);
        assert_ne!(h.flags & dkrt_sdk::HEALTH_STORE, 0);
        assert_eq!((h.store_used, h.store_free), (0, free));
    }
    // the first upload writes the headers: the same views follow
    assert_eq!(upload("modules/a.dkm", &pattern(1000, 3)), OK);
    unsafe {
        crate::store::figures_stale();
        let mut list = [const { unsafe { core::mem::zeroed::<DkrtStoreEntry>() } }; 4];
        assert_eq!(crate::store::list(b"modules/\0".as_ptr(), list.as_mut_ptr(), 4), 1);
        assert!(crate::store::seen().unwrap().0 > 0);
    }
}

#[test]
fn a_foreign_area_has_no_store_for_the_list_and_the_figures() {
    let _f = Fixture::new();
    card(|c| {
        c.sectors.insert(L.first + 5, [0x55; 512]);
    });
    unsafe {
        card_gate::check();
        crate::store::figures_stale();
        let mut list = [const { unsafe { core::mem::zeroed::<DkrtStoreEntry>() } }; 4];
        assert!(crate::store::list(b"modules/\0".as_ptr(), list.as_mut_ptr(), 4) < 0);
        assert_eq!(crate::store::seen(), None);
    }
}

/// The three views of the store: the module list (`dkrt_scan`'s), the HEALTH figures and the SysEx LIST (INFO and the upload steps use the same state).
fn views() -> (i32, Option<(u32, u32)>, i32) {
    unsafe {
        crate::store::figures_stale();
        let mut list = [const { unsafe { core::mem::zeroed::<DkrtStoreEntry>() } }; 4];
        let l = crate::store::list(b"modules/\0".as_ptr(), list.as_mut_ptr(), 4);
        (l, crate::store::seen(), list_cmd(None).0)
    }
}

/// A header sector with our magic and version 4 and a CRC that does not match.
fn torn_header() -> [u8; 512] {
    let mut h = [0u8; 512];
    h[..4].copy_from_slice(&dkrt_sdk::store::MAGIC.to_be_bytes());
    h[4..8].copy_from_slice(&dkrt_sdk::store::VERSION.to_be_bytes());
    h[100] = 7;
    h
}

/// The guard of the empty-store view: only a card that may hold one, with both header sectors blank (or of another version), is an empty store. Every
/// other case keeps data safe: no store for the list, the figures and the LIST.
#[test]
fn only_a_blank_header_pair_on_a_card_that_may_hold_a_store_is_an_empty_store() {
    let hb = L.header_b();
    // not checked yet (UNKNOWN) and blank: an empty store, as before the card check ran
    let _f = Fixture::new();
    let (l, seen, lst) = views();
    assert_eq!((l, lst), (0, OK), "unchecked, blank");
    assert!(seen.is_some());
    // the figures read under UNKNOWN do not outlive the decision: a card found foreign later shows no store without an upload in between
    card(|c| {
        c.sectors.insert(L.first + 5, [0x55; 512]);
    });
    unsafe { card_gate::check() };
    assert_eq!(card_gate::state(), dkrt_sdk::CARD_FOREIGN);
    unsafe {
        assert_eq!(crate::store::seen(), None, "a stale cache is not shown for a card decided foreign");
    }
    drop(_f);
    // unreadable and too small cards
    for state in [dkrt_sdk::CARD_UNREADABLE, dkrt_sdk::CARD_TOO_SMALL] {
        let _f = Fixture::new();
        card_gate::info().state = state;
        let (l, seen, lst) = views();
        assert!(l < 0 && seen.is_none() && lst == p::E_NOSTORE, "state {state}: {l} {seen:?} {lst}");
    }
    // both headers torn: a header is there, no copy is usable
    {
        let _f = Fixture::new();
        card(|c| {
            c.sectors.insert(L.first, torn_header());
            c.sectors.insert(L.first + hb, torn_header());
        });
        let (l, seen, lst) = views();
        assert!(l < 0 && seen.is_none() && lst == p::E_NOSTORE, "torn: {l} {seen:?} {lst}");
    }
    // a non-blank header sector that is not a store header (somebody's data), checked or not
    for check in [false, true] {
        let _f = Fixture::new();
        card(|c| {
            c.sectors.insert(L.first + hb, [0x55; 512]);
        });
        if check {
            unsafe { card_gate::check() };
        }
        let (l, seen, lst) = views();
        assert!(l < 0 && seen.is_none() && lst == p::E_NOSTORE, "foreign header (checked {check}): {l} {seen:?} {lst}");
    }
    // a header of another version is no store, as before: an empty one
    {
        let _f = Fixture::new();
        let mut old = [0u8; 512];
        old[..4].copy_from_slice(&dkrt_sdk::store::MAGIC.to_be_bytes());
        old[4..8].copy_from_slice(&(dkrt_sdk::store::VERSION - 1).to_be_bytes());
        card(|c| {
            c.sectors.insert(L.first, old);
        });
        let (l, seen, lst) = views();
        assert!(l == 0 && seen.is_some() && lst == OK, "old version: {l} {seen:?} {lst}");
    }
}

/// The two selection copies as the card holds them, decoded.
fn selection_copies() -> [Option<dkrt_sdk::sel::Record>; 2] {
    let at = |s: u32| card(|c| c.sectors.get(&s).copied()).and_then(|b| dkrt_sdk::sel::decode(&b));
    [at(L.sel_a()), at(L.sel_b())]
}

/// SELECT after the boot counter's record: both copies are written (two card writes), with the next sequence number and the counter of the record kept.
#[test]
fn a_selection_write_keeps_the_boot_counter_takes_the_next_sequence_number_and_writes_both_copies() {
    let _f = Fixture::new();
    unsafe { card_gate::check() };
    let mut r = dkrt_sdk::sel::Record::empty();
    r.boot_counter = 1;
    assert_eq!(unsafe { crate::selection::update(r, 0, false) }, OK, "the boot counter's record: copy A, seq 1");
    assert_eq!(card(|c| c.writes), 1);
    let mut body = vec![1u8];
    body.extend(zpath("modules/a.dkm"));
    assert_eq!(send(store_cmds::h_select, &body), OK);
    assert_eq!(card(|c| c.writes), 3, "both copies");
    let [a, b] = selection_copies();
    let (a, b) = (a.expect("copy A is valid"), b.expect("copy B is valid"));
    assert_eq!((a.seq, a.boot_counter, a.count), (2, 1, 1));
    assert_eq!((b.seq, b.boot_counter, b.count), (2, 1, 1));
    assert_eq!(&a.paths[0][..14], b"modules/a.dkm\0");
    assert_eq!(a.paths, b.paths);
}

/// A selection record, as the boot of a fresh runtime leaves it on a card.
fn put_selection_record() {
    let mut b = [0u8; 512];
    dkrt_sdk::sel::encode(&dkrt_sdk::sel::Record::empty(), &mut b);
    card(|c| {
        c.sectors.insert(L.sel_a(), b);
    });
}

/// The store a previous runtime left (version 3 headers, a table, a file) beside the runtime's selection record: it reads as no store, the check and the
/// views write nothing, and the first upload makes a version 4 store in both headers that lists only the new file.
#[test]
fn a_version_3_store_is_an_empty_store_that_the_first_upload_turns_into_version_4() {
    let _f = Fixture::new();
    let hb = L.header_b();
    // the previous runtime's store as it wrote it: the layout of version 4 with version 3 in the header, a table of one entry and a CRC of both that holds
    let mut table = [0u8; 512];
    table[..9].copy_from_slice(b"modules/o");
    table[160..164].copy_from_slice(&2u32.to_be_bytes()); // first sector
    table[164..168].copy_from_slice(&512u32.to_be_bytes()); // size
    let table_crc = unsafe { dkrt_sdk::store::crc32_of(table.as_ptr(), 512) };
    let hdr = dkrt_sdk::store::Hdr { count: 1, table_sectors: 1, table_first: 1, table_crc, seq: 1, used: 3, area: L.sectors };
    let mut old = [0u8; 512];
    unsafe { dkrt_sdk::store::encode_header(&hdr, old.as_mut_ptr()) };
    old[4..8].copy_from_slice(&(dkrt_sdk::store::VERSION - 1).to_be_bytes());
    let crc = unsafe { dkrt_sdk::store::crc32_of(old.as_ptr(), 508) };
    old[508..].copy_from_slice(&crc.to_be_bytes());
    let mut as_v4 = old;
    unsafe { dkrt_sdk::store::encode_header(&hdr, as_v4.as_mut_ptr()) };
    assert!(unsafe { dkrt_sdk::store::parse_header(as_v4.as_ptr()) }.is_some(), "the same fields with version 4 are a header");
    put_selection_record();
    card(|c| {
        c.sectors.insert(L.first, old);
        c.sectors.insert(L.first + hb, old);
        c.sectors.insert(L.first + 1, table);
        c.sectors.insert(L.first + 2, [0x44; 512]); // its file
    });
    let writes = card(|c| c.writes);
    unsafe { card_gate::check() };
    assert_eq!(card_gate::state(), dkrt_sdk::CARD_OURS);
    let (l, seen, lst) = views();
    assert!(l == 0 && seen.is_some() && lst == OK, "no store: {l} {seen:?} {lst}");
    assert!(held().is_empty());
    assert_eq!(card(|c| c.writes), writes, "the check and the views write nothing");
    assert_eq!(upload("modules/a.dkm", &pattern(1000, 3)), OK);
    for sector in [L.first, L.first + hb] {
        let h = card(|c| c.sectors[&sector]);
        assert_eq!(u32::from_be_bytes(h[4..8].try_into().unwrap()), dkrt_sdk::store::VERSION, "header at {sector} is version 4");
    }
    assert_eq!(held().keys().collect::<Vec<_>>(), ["modules/a.dkm"], "only the new file");
}

/// A claim cut inside its first write leaves header B torn and header A blank. With the runtime's selection record the card is ours, but the torn header is
/// no store: an upload is refused and writes nothing; CLEAR makes the store, and the next upload commits.
#[test]
fn a_torn_header_b_with_a_blank_header_a_is_no_store_until_clear_makes_one() {
    let _f = Fixture::new();
    let mut torn_b = torn_header();
    torn_b[256..].fill(0);
    put_selection_record();
    card(|c| {
        c.sectors.insert(L.first + L.header_b(), torn_b);
    });
    unsafe { card_gate::check() };
    assert_eq!(card_gate::state(), dkrt_sdk::CARD_OURS);
    let writes = card(|c| c.writes);
    let blob = pattern(3000, 31);
    assert_eq!(begin("modules/b.dkm", blob.len(), slot(&blob)), p::E_NOSTORE);
    assert_eq!(card(|c| c.writes), writes, "the refused upload wrote nothing");
    assert_eq!(clear_with(CLEAR_MAGIC), OK);
    assert_eq!(upload("modules/b.dkm", &blob), OK, "the next upload commits");
    assert!(held().contains_key("modules/b.dkm"));
}

// ---- power cuts -----------------------------------------------------------------------------------------------------------------------

/// A cut or a torn sector at each card write of one upload leaves the old table or the new one, files that read back with their hashes, and a store that
/// takes the next write.
fn cut_sweep(tear: bool) {
    let _f = Fixture::new();
    let (a, b, c) = (pattern(3000, 3), pattern(5000, 5), pattern(700, 9));
    assert_eq!(upload("keep/a", &a), OK);
    let before = held();
    let snapshot = card(|c| c.sectors.clone());
    // how many card writes one upload of b takes
    unsafe { write_volatile(addr_of_mut!(sw::DKRT_UPLOAD_WRITES), 0) };
    assert_eq!(upload("keep/b", &b), OK);
    let n = unsafe { read_volatile(addr_of!(sw::DKRT_UPLOAD_WRITES)) };
    let after = held();
    assert!(n >= 3, "data, table and headers: {n}");
    assert_ne!(before, after);
    for k in 1..=n {
        card(|c| c.sectors = snapshot.clone());
        crate::store::test_forget_checked();
        unsafe {
            write_volatile(addr_of_mut!(sw::DKRT_UPLOAD_WRITES), 0);
            if tear {
                write_volatile(addr_of_mut!(sw::DKRT_UPLOAD_TEAR), crate::safe_mode::Knob(k));
            } else {
                write_volatile(addr_of_mut!(sw::DKRT_UPLOAD_CUT), k);
            }
        }
        let r = upload("keep/b", &b);
        unsafe { rest() };
        let now = held();
        assert!(now == before || now == after, "{} at write {k} of {n}: {now:?}", if tear { "tear" } else { "cut" });
        if r == OK {
            assert_eq!(now, after, "an upload that said ok is there (write {k})");
        }
        for (path, (_, sha)) in &now {
            assert_eq!(verify_cmd(path, None), (OK, *sha), "{path} after the cut at write {k}");
        }
        assert_eq!(upload("keep/c", &c), OK, "the store takes a write after the cut at {k}");
        let h = held();
        assert!(h.contains_key("keep/a") && h.contains_key("keep/c"));
    }
}

#[test]
fn a_write_cut_at_any_card_write_leaves_the_old_table_or_the_new_one() {
    cut_sweep(false);
}

#[test]
fn a_sector_torn_at_any_card_write_leaves_the_old_table_or_the_new_one() {
    cut_sweep(true);
}

// ---- the Storage contract table against the real store -----------------------------------------------------------------------------------

/// The store's side of a device: files of the table under the device's roots, then every row of `tests/storage/contract.toml` through `PLATFORM`.
/// A device with the firmware file system serves its samples root from there (`dt2_sample_reads_*`): only the store root runs for it here.
fn contract_on_the_real_store(profile: DeviceProfile) {
    let _f = Fixture::new();
    let _p = ProfileGuard::set(profile);
    let (files, cases) = rows(include_str!("../../../tests/storage/contract.toml"));
    for f in &files {
        let root = f["root"].as_str();
        if root == "samples" && !profile.samples_in_store {
            continue;
        }
        let path = if root == "samples" { std::format!("samples/{}", f["path"]) } else { f["path"].clone() };
        assert_eq!(upload(&path, &ramp(f["size"].parse().unwrap())), OK, "{f:?}");
    }
    let mut checked = 0;
    for case in &cases {
        if case["root"] == "samples" && !profile.samples_in_store {
            continue;
        }
        checked += 1;
        let got = answer(&PLATFORM, case);
        let want = &case["expect"];
        let want = if want == "profile" { profile.sample_path_max.to_string() } else { want.clone() };
        assert_eq!(got.split_whitespace().next(), want.split_whitespace().next(), "{} on {profile:?}", case["id"]);
        if !want.starts_with("list") {
            assert_eq!(got, want, "{} on {profile:?}", case["id"]);
        }
    }
    assert!(checked >= 10);
}

#[test]
fn the_contract_table_holds_for_the_real_store_of_a_device_with_its_samples_in_it() {
    contract_on_the_real_store(profiles::DN2);
}

#[test]
fn the_contract_table_holds_for_the_real_store_root_of_a_device_with_the_firmware_file_system() {
    contract_on_the_real_store(profiles::DT2);
}

// ---- the routes -----------------------------------------------------------------------------------------------------------------------

/// The decider of the writer for a row.
fn writer_route(r: &RouteRow) -> (Backend, String) {
    let f = FakePlatform::new(r.profile);
    f.staged.set(r.staged);
    match root_route(&f, r.root) {
        Ok((prefix, false)) => (Backend::Store, std::format!("{}{}", String::from_utf8_lossy(prefix), r.path)),
        Ok((_, true)) => (Backend::PlusDrive, r.path.to_string()),
        Err(_) => (Backend::Refused, String::new()),
    }
}

#[test]
fn the_writer_routes_agree_with_the_route_table() {
    let mut rows = 0;
    for r in ROUTES.iter().filter(|r| r.op == Op::Write) {
        rows += 1;
        let (backend, key) = writer_route(r);
        assert_eq!((backend, key.as_str()), (r.backend, r.key), "{r:?}");
    }
    assert!(rows >= 8);
}

#[test]
fn a_dt2_sample_write_goes_to_the_plusdrive_adapter_and_never_to_the_store() {
    let _f = Fixture::new();
    let _pd = PdwGuard::install();
    unsafe {
        crate::storage::backends::plusdrive::tick();
        sw::tick()
    }; // the adapter's boot recovery runs in the UI pass
    let bytes = pattern(1000, 7);
    let writes = card(|c| c.writes);
    assert_eq!(sdk_put(SAMPLES, "KICKS/K1", &bytes, 0), OK);
    pdw(|a| {
        assert_eq!(a.files.get(b"KICKS/K1".as_slice()), Some(&bytes));
        assert_eq!(a.calls.iter().filter(|c| *c == "begin KICKS/K1 1000 0").count(), 1, "{:?}", a.calls);
    });
    assert_eq!(card(|c| c.writes), writes, "the store's card was not written");
    assert!(held().is_empty());
    assert_eq!(sdk_delete(SAMPLES, "KICKS/K1"), OK);
    pdw(|a| assert!(a.files.is_empty()));
    // the store root of the same device is the store
    let calls = pdw(|a| a.calls.len());
    assert_eq!(sdk_put(STORE, "cases/a", &bytes, 0), OK);
    assert_eq!(held()["cases/a"], (1000, slot(&bytes)));
    assert_eq!(pdw(|a| a.calls.len()), calls, "the adapter heard nothing of it");
}

#[test]
fn a_dt2_without_the_plusdrive_adapter_has_no_samples_root_to_write() {
    let _f = Fixture::new();
    assert_eq!(sdk_put(SAMPLES, "KICKS/K1", &pattern(100, 7), 0), dkrt_sdk::E_CONTRACT);
    assert_eq!(sdk_delete(SAMPLES, "KICKS/K1"), dkrt_sdk::E_CONTRACT);
    assert!(held().is_empty());
}

#[test]
fn a_dn2_sample_write_goes_to_the_samples_folder_of_the_store_and_never_to_the_adapter() {
    let _f = Fixture::new();
    let _pd = PdwGuard::install(); // present, and not used
    let _p = ProfileGuard::set(profiles::DN2);
    let bytes = pattern(1000, 7);
    assert_eq!(sdk_put(SAMPLES, "KICKS/K1", &bytes, 0), OK);
    assert_eq!(held()["samples/KICKS/K1"], (1000, slot(&bytes)));
    assert_eq!(list_cmd(Some("samples/")).2.len(), 1);
    pdw(|a| assert!(a.calls.is_empty(), "{:?}", a.calls));
    let st = unsafe { PLATFORM.stat(ROOT_SAMPLES, b"KICKS/K1") }.unwrap();
    assert_eq!((st.kind, st.size, st.hash_valid, st.hash), (EntryKind::File, 1000, true, slot(&bytes)));
    assert_eq!(sdk_delete(SAMPLES, "KICKS/K1"), OK);
    assert!(held().is_empty());
}

#[test]
fn a_dt2_sample_read_goes_to_the_firmware_file_system_and_never_to_the_store() {
    let _f = Fixture::new();
    let on_fs = pattern(300, 7);
    let _fs = FsGuard::install(&[("/KICKS/K1", &on_fs)]);
    // the store has the same name under its samples folder, with other bytes: it must not be seen
    assert_eq!(upload("samples/KICKS/K1", &pattern(100, 3)), OK);
    let reads = card(|c| c.reads);
    let st = unsafe { PLATFORM.stat(ROOT_SAMPLES, b"KICKS/K1") }.unwrap();
    assert_eq!((st.size, st.hash_kind, st.hash_valid), (300, HASH_ELEKTRON32, true));
    assert_eq!(st.hash[..4], crate::host_io::fs_odd_hash(300).to_be_bytes(), "the firmware's odd hash, not the store's");
    let mut all = vec![0u8; 300];
    assert_eq!(unsafe { PLATFORM.read_all(ROOT_SAMPLES, b"KICKS/K1", all.as_mut_ptr(), 300) }, Ok(300));
    assert_eq!(all, on_fs);
    assert_eq!(unsafe { PLATFORM.stat(ROOT_SAMPLES, b"KICKS/none") }, Err(dkrt_sdk::E_NOT_FOUND));
    assert_eq!(card(|c| c.reads), reads, "no card read: the firmware serves the samples");
    fs(|f| assert!(f.opens.iter().all(|p| p.starts_with(b"/KICKS/")) && f.opens.len() >= 3, "{:?}", f.opens));
    // the store root of the same device is the store
    let opens = fs(|f| f.opens.len());
    let st = unsafe { PLATFORM.stat(ROOT_STORE, b"samples/KICKS/K1") }.unwrap();
    assert_eq!(st.size, 100);
    assert_eq!(fs(|f| f.opens.len()), opens);
}

#[test]
fn a_dn2_sample_read_goes_to_the_store_and_never_to_the_firmware_file_system() {
    let _f = Fixture::new();
    let _p = ProfileGuard::set(profiles::DN2);
    let _fs = FsGuard::install(&[("/KICKS/K1", &pattern(300, 7))]);
    let bytes = pattern(100, 3);
    assert_eq!(upload("samples/KICKS/K1", &bytes), OK);
    let st = unsafe { PLATFORM.stat(ROOT_SAMPLES, b"KICKS/K1") }.unwrap();
    assert_eq!((st.size, st.hash), (100, slot(&bytes)));
    let mut all = vec![0u8; 100];
    assert_eq!(unsafe { PLATFORM.read_all(ROOT_SAMPLES, b"KICKS/K1", all.as_mut_ptr(), 100) }, Ok(100));
    assert_eq!(all, bytes);
    assert!(fs(|f| f.opens.is_empty()), "the firmware file system was not asked");
}

// ---- one lister -----------------------------------------------------------------------------------------------------------------------

/// The same folder through every lister of the table: the SysEx LIST, the runtime's `store::list` (which the module scan and the loader's provider search
/// use), and the `Storage` listing. The module scan (`dkrt_scan`) calls `store::list` too; dkrt_scan segfaults on the host (cause not found); covered again when RS-S S2b replaces the listers with one. Files with a path that fits 63 bytes and at least one byte are what all of them give, so that is the
/// folder here.
#[test]
fn every_lister_gives_the_same_entries_for_the_same_folder() {
    let _f = Fixture::new();
    let files: [(&str, usize); 6] =
        [("modules/a.dkm", 700), ("modules/b.dkm", 1500), ("modules/note.txt", 100), ("services/s.dks", 300), ("modules/sub/c.dkm", 40), ("other/z", 20)];
    for (path, size) in files {
        assert_eq!(upload(path, &pattern(size, 7)), OK, "{path}");
    }
    // the folder `modules/`, by path: files directly in it
    let want: BTreeMap<String, u32> = [("modules/a.dkm", 700), ("modules/b.dkm", 1500), ("modules/note.txt", 100)].map(|(p, s)| (p.to_string(), s)).into();

    // 1: SysEx LIST with the prefix `modules/` (every entry under it, the nested file too)
    let (r, count, rows) = list_cmd(Some("modules/"));
    assert_eq!((r, count), (OK, 4));
    let sysex: BTreeMap<String, (u32, [u8; 32])> = rows.into_iter().map(|(p, s, h)| (p, (s, h))).collect();
    let direct = |m: &BTreeMap<String, (u32, [u8; 32])>| m.iter().filter(|(p, _)| p.matches('/').count() == 1).map(|(p, (s, _))| (p.clone(), *s)).collect::<BTreeMap<_, _>>();
    assert_eq!(direct(&sysex), want);

    // 2: store::list with the same prefix
    let mut out = unsafe { core::mem::zeroed::<[DkrtStoreEntry; 8]>() };
    let n = unsafe { crate::store::list(b"modules/\0".as_ptr(), out.as_mut_ptr(), 8) };
    assert_eq!(n, 4);
    let cname = |p: &[u8; 64]| String::from_utf8(p[..p.iter().position(|b| *b == 0).unwrap()].to_vec()).unwrap();
    let listed: BTreeMap<String, u32> = out[..4].iter().map(|e| (cname(&e.path), e.size)).collect();
    assert_eq!(listed.iter().filter(|(p, _)| p.matches('/').count() == 1).map(|(p, s)| (p.clone(), *s)).collect::<BTreeMap<_, _>>(), want);

    // 3: the loader's provider search lists through the same call
    let mut out2 = unsafe { core::mem::zeroed::<[DkrtStoreEntry; 8]>() };
    let n2 = unsafe { crate::package::loader::discover::Cards::list(&mut crate::package::loader::RealImages, b"modules/\0".as_ptr(), out2.as_mut_ptr(), 8) };
    assert_eq!(n2, n);
    assert_eq!(out2[..4].iter().map(|e| (cname(&e.path), e.size, e.first)).collect::<Vec<_>>(), out[..4].iter().map(|e| (cname(&e.path), e.size, e.first)).collect::<Vec<_>>());

    // 4: the Storage listing of the folder (its children: files, and the folder `sub`), with the hashes of the table
    let kids = storage_list(&PLATFORM, ROOT_STORE, b"modules").unwrap();
    let files_here: BTreeMap<String, (u32, [u8; 32])> =
        kids.iter().filter(|(_, st)| st.kind == EntryKind::File).map(|(n, st)| (std::format!("modules/{}", String::from_utf8_lossy(n)), (st.size, st.hash))).collect();
    assert_eq!(files_here.iter().map(|(p, (s, _))| (p.clone(), *s)).collect::<BTreeMap<_, _>>(), want);
    for (path, (_, hash)) in &files_here {
        assert_eq!(*hash, sysex[path].1, "{path}: the same hash slot as LIST");
    }
    assert!(kids.iter().any(|(n, st)| n == b"sub" && st.kind == EntryKind::Dir));
}

/// `store_find` of a name under `modules/`: the status and the path it found.
fn find_module(name: &str) -> (i32, String) {
    let mut out = [0u8; 64];
    let r = unsafe { crate::package::scan::store_find(b"modules/", name.as_bytes(), &mut out) };
    let end = out.iter().position(|b| *b == 0).unwrap_or(64);
    (r, String::from_utf8_lossy(&out[..end]).into_owned())
}

#[test]
fn store_find_reads_the_whole_store_and_is_not_found_only_after_all_of_it() {
    let _f = Fixture::new();
    // more module files than the listing's cap, the wanted one past it
    for i in 0..(dkrt_sdk::SCAN_MAX + 8) {
        assert_eq!(upload(&std::format!("modules/m{i:02}/f{i:02}.dkm"), &pattern(600, 3)), OK);
    }
    let last = dkrt_sdk::SCAN_MAX + 7;
    let (r, path) = find_module(&std::format!("f{last:02}.dkm"));
    assert_eq!(path, std::format!("modules/m{last:02}/f{last:02}.dkm"));
    assert_eq!(r as usize, path.len());
    assert_eq!(find_module("f00.dkm").0 as usize, "modules/m00/f00.dkm".len());
    assert_eq!(find_module("nothing.dkm").0, dkrt_sdk::E_NOT_FOUND);
}

#[test]
fn store_find_gives_the_error_of_a_busy_store_and_not_a_missing_file() {
    let _f = Fixture::new();
    assert_eq!(upload("modules/m/a-x.dkm", &pattern(600, 3)), OK);
    assert!(sw::try_lock(), "the store lock is free");
    let (r, _) = find_module("a-x.dkm");
    sw::unlock();
    assert_eq!(r, dkrt_sdk::E_BUSY);
    assert_eq!(find_module("a-x.dkm").0 as usize, "modules/m/a-x.dkm".len());
}
