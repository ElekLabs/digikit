//! The version 3 store on a card in RAM: the format, the table stream, free-space reuse, and what a power cut can leave.
extern crate std;
use super::*;
use crate::{E_BAD_STORE, E_NOT_FOUND};
use std::sync::Mutex;
use std::vec::Vec;

const FIRST: u32 = 5000;
const SECTORS: u32 = 4096;
const LAY: Layout = Layout { first: FIRST, sectors: SECTORS };

static CARD: Mutex<Vec<u8>> = Mutex::new(Vec::new());
static LOCK: Mutex<()> = Mutex::new(());

/// The tests share one card: one at a time (a failed test must not poison the rest).
fn lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}
/// Writes allowed before every further one fails (`u32::MAX`: no limit) and writes done.
static CUT: Mutex<(u32, u32)> = Mutex::new((u32::MAX, 0));

unsafe extern "C" fn rd(sector: u32, bytes: u32, buf: *mut u8) -> i32 {
    let c = CARD.lock().unwrap();
    let at = (sector - FIRST) as usize * 512;
    core::ptr::copy_nonoverlapping(c.as_ptr().add(at), buf, bytes as usize);
    0
}

unsafe extern "C" fn wr(sector: u32, bytes: u32, buf: *mut u8) -> i32 {
    {
        let mut k = CUT.lock().unwrap();
        if k.1 >= k.0 {
            return -1;
        }
        k.1 += 1;
    }
    assert!(LAY.write_allowed(sector, bytes), "a write outside the store: {sector} {bytes}");
    let mut c = CARD.lock().unwrap();
    let at = (sector - FIRST) as usize * 512;
    core::ptr::copy_nonoverlapping(buf, c.as_mut_ptr().add(at), bytes as usize);
    0
}

const IO: Io = Io { rd, wr };

#[repr(C, align(16))]
struct Buf<const N: usize>([u8; N]);

fn fresh() {
    *CARD.lock().unwrap() = std::vec![0u8; SECTORS as usize * 512];
    *CUT.lock().unwrap() = (u32::MAX, 0);
}

fn cstr(p: &str) -> Vec<u8> {
    let mut v = p.as_bytes().to_vec();
    v.push(0);
    v
}

fn entry(first: u32, size: u32, kind: u8) -> Entry {
    Entry { first, size, kind, sha: [(first & 0xff) as u8; 32] }
}

/// A scratch set: 16-byte aligned buffers.
struct Work {
    a: Box<Buf<4096>>,
    b: Box<Buf<4096>>,
    s: Box<Buf<512>>,
}

use std::boxed::Box;

fn work() -> Work {
    Work { a: Box::new(Buf([0; 4096])), b: Box::new(Buf([0; 4096])), s: Box::new(Buf([0; 512])) }
}

/// The state of the store as `open` sees it (no table: a fresh one).
fn state(w: &mut Work) -> Result<Table, i32> {
    unsafe { open(&IO, &LAY, w.s.0.as_mut_ptr(), w.a.0.as_mut_ptr(), 4096, true) }
}

fn current(w: &mut Work) -> Hdr {
    match state(w) {
        Ok(t) => t.h,
        Err(_) => Hdr::empty(&LAY),
    }
}

/// Commit an insert of `path` with data at `first` (`size` bytes).
fn insert(w: &mut Work, path: &str, first: u32, size: u32, extra: &[(u32, u32)]) -> Result<Committed, i32> {
    let cur = current(w);
    let mut e = [0u8; ENTRY];
    let p = cstr(path);
    unsafe { encode_entry(p.as_ptr(), &entry(first, size, KIND_FILE), e.as_mut_ptr()) };
    unsafe { commit(&IO, &LAY, &cur, w.a.0.as_mut_ptr(), w.b.0.as_mut_ptr(), 1024, w.s.0.as_mut_ptr(), Edit::Insert(e.as_ptr()), extra) }
}

fn remove(w: &mut Work, path: &str) -> Result<Committed, i32> {
    let cur = current(w);
    let p = cstr(path);
    unsafe { commit(&IO, &LAY, &cur, w.a.0.as_mut_ptr(), w.b.0.as_mut_ptr(), 1024, w.s.0.as_mut_ptr(), Edit::Remove(p.as_ptr()), &[]) }
}

/// Plan and commit a rename of `from` to `to`.
fn rename(w: &mut Work, from: &str, to: &str) -> Result<Committed, i32> {
    let cur = current(w);
    let (f, t) = (cstr(from), cstr(to));
    let plan = unsafe { plan_rename(&IO, &LAY, &cur, w.a.0.as_mut_ptr(), 4096, f.as_ptr(), t.as_ptr()) }?;
    unsafe { commit(&IO, &LAY, &cur, w.a.0.as_mut_ptr(), w.b.0.as_mut_ptr(), 1024, w.s.0.as_mut_ptr(), plan.edit(f.as_ptr()), &[]) }
}

/// Every entry: path, first sector, size, kind, sha256.
fn entries(w: &mut Work) -> Vec<(std::string::String, Entry)> {
    let h = current(w);
    let mut out = Vec::new();
    unsafe {
        scan(&IO, &LAY, &h, w.a.0.as_mut_ptr(), 4096, |_, e| {
            let n = path_len(e);
            out.push((std::string::String::from_utf8_lossy(core::slice::from_raw_parts(e, n)).into_owned(), entry_of(e)));
            true
        });
    }
    out
}

fn paths(w: &mut Work) -> Vec<(std::string::String, u32)> {
    let h = current(w);
    let mut out = Vec::new();
    unsafe {
        scan(&IO, &LAY, &h, w.a.0.as_mut_ptr(), 4096, |_, e| {
            let n = path_len(e);
            out.push((std::string::String::from_utf8_lossy(core::slice::from_raw_parts(e, n)).into_owned(), be32(e.add(O_FIRST))));
            true
        });
    }
    out
}

#[test]
fn crc32_matches_the_zlib_check_value() {
    assert_eq!(crc32_of(b"123456789".as_ptr(), 9), 0xcbf4_3926);
}

#[test]
fn the_geometry_is_the_documented_one() {
    assert_eq!(TAIL, 35);
    assert_eq!(LAY.header_b(), SECTORS - 35);
    assert_eq!(LAY.limit(), SECTORS - 35);
    assert_eq!(LAY.set_a(), FIRST + SECTORS - 34);
    assert_eq!(LAY.set_b(), FIRST + SECTORS - 18);
    assert_eq!(LAY.set_b() + 16, LAY.sel_a());
    assert_eq!(LAY.sel_a(), FIRST + SECTORS - 2);
    assert_eq!(LAY.sel_b(), FIRST + SECTORS - 1);
    assert_eq!(table_sectors(0), 0);
    assert_eq!(table_sectors(1), 1);
    assert_eq!(table_sectors(2), 1);
    assert_eq!(table_sectors(3), 2);
    // the card writes of the runtime are allowed only inside the area
    assert!(LAY.write_allowed(FIRST, 512));
    assert!(LAY.write_allowed(FIRST + SECTORS - 1, 512));
    assert!(LAY.write_allowed(FIRST, SECTORS * 512));
    assert!(!LAY.write_allowed(FIRST - 1, 512));
    assert!(!LAY.write_allowed(FIRST + SECTORS, 512));
    assert!(!LAY.write_allowed(FIRST + SECTORS - 1, 1024));
    assert!(!LAY.write_allowed(FIRST, 0));
    assert!(!LAY.write_allowed(FIRST, 100));
    assert!(!LAY.write_allowed(u32::MAX, 512));
    assert!(!LAY.write_allowed(0, 512));
}

#[test]
fn an_empty_area_is_no_store_and_a_commit_makes_one() {
    let _g = lock();
    fresh();
    let mut w = work();
    assert_eq!(state(&mut w).err(), Some(E_NOT_FOUND));
    let c = insert(&mut w, "modules/a.dkm", 10, 1000, &[(10, 2)]).unwrap();
    assert_eq!((c.h.count, c.h.seq, c.h.table_sectors), (1, 1, 1));
    assert_eq!(c.h.used, 1 + 1 + 2);
    let t = state(&mut w).unwrap();
    assert_eq!((t.h, t.copy, t.agree), (c.h, 0, true));
    let p = cstr("modules/a.dkm");
    let f = unsafe { find(&IO, &LAY, &t.h, w.a.0.as_mut_ptr(), 4096, p.as_ptr()) }.unwrap().unwrap();
    assert_eq!((f.0, f.1.first, f.1.size), (0, 10, 1000));
    let q = cstr("store/zzz");
    assert!(unsafe { find(&IO, &LAY, &t.h, w.a.0.as_mut_ptr(), 4096, q.as_ptr()) }.unwrap().is_none());
}

#[test]
fn the_table_is_sorted_by_first_sector_then_path_and_a_path_is_replaced() {
    let _g = lock();
    fresh();
    let mut w = work();
    insert(&mut w, "b", 30, 512, &[]).unwrap();
    insert(&mut w, "a", 20, 512, &[]).unwrap();
    insert(&mut w, "zero2", 0, 0, &[]).unwrap();
    insert(&mut w, "zero1", 0, 0, &[]).unwrap();
    insert(&mut w, "c", 25, 512, &[]).unwrap();
    let got: Vec<_> = paths(&mut w).into_iter().map(|p| p.0).collect();
    assert_eq!(got, ["zero1", "zero2", "a", "c", "b"]);
    // the same path again replaces the entry and frees its extent
    let c = insert(&mut w, "a", 40, 1024, &[]).unwrap();
    assert_eq!(c.removed.unwrap().first, 20);
    let got: Vec<_> = paths(&mut w);
    assert_eq!(got.iter().map(|p| p.0.as_str()).collect::<Vec<_>>(), ["zero1", "zero2", "c", "b", "a"]);
    assert_eq!(current(&mut w).count, 5);
}

/// Commit an insert of the folder entry `path`.
fn mkdir(w: &mut Work, path: &str) -> Result<Committed, i32> {
    let cur = current(w);
    let mut e = [0u8; ENTRY];
    let p = cstr(path);
    unsafe { encode_entry(p.as_ptr(), &entry(0, 0, KIND_DIR), e.as_mut_ptr()) };
    unsafe { commit(&IO, &LAY, &cur, w.a.0.as_mut_ptr(), w.b.0.as_mut_ptr(), 1024, w.s.0.as_mut_ptr(), Edit::Insert(e.as_ptr()), &[]) }
}

#[test]
fn a_folder_entry_goes_when_something_is_stored_inside_it() {
    let _g = lock();
    fresh();
    let mut w = work();
    mkdir(&mut w, "samples").unwrap();
    mkdir(&mut w, "samples/drums").unwrap();
    mkdir(&mut w, "other").unwrap();
    // a folder inside a folder makes the outer one implied: only the innermost folder entry is kept
    let p: Vec<_> = paths(&mut w).into_iter().map(|x| x.0).collect();
    assert_eq!(p, ["other", "samples/drums"]);
    // a file in samples/drums/: its folder entry goes, the unrelated one stays
    insert(&mut w, "samples/drums/kick.wav", 10, 1000, &[]).unwrap();
    let p: Vec<_> = paths(&mut w).into_iter().map(|x| x.0).collect();
    assert_eq!(p, ["other", "samples/drums/kick.wav"]);
    // a name that only starts like a folder is not inside it: "other" stays when "others/x" is stored
    insert(&mut w, "others/x", 20, 10, &[]).unwrap();
    assert_eq!(paths(&mut w).len(), 3);
    // the table is intact
    assert!(state(&mut w).is_ok());
    // deleting the last file leaves no folder entry: the folder was implied
    remove(&mut w, "samples/drums/kick.wav").unwrap();
    let p: Vec<_> = paths(&mut w).into_iter().map(|x| x.0).collect();
    assert_eq!(p, ["other", "others/x"]);
}

#[test]
fn a_remove_drops_the_entry_and_the_space_is_used_again() {
    let _g = lock();
    fresh();
    let mut w = work();
    // three files of 100 sectors from sector 10, 110, 210
    insert(&mut w, "f1", 10, 100 * 512, &[]).unwrap();
    insert(&mut w, "f2", 110, 100 * 512, &[]).unwrap();
    insert(&mut w, "f3", 210, 100 * 512, &[]).unwrap();
    let h = current(&mut w);
    let first_fit_then = |w: &mut Work, h: &Hdr, n: u32| unsafe { first_fit(&IO, &LAY, h, w.a.0.as_mut_ptr(), 4096, n, &[]) }.unwrap();
    // gaps: [1, 10) minus the table, the table's neighbours: a file of 150 sectors goes after f3
    let at = first_fit_then(&mut w, &h, 150).unwrap();
    assert!(at >= 310, "{at}");
    let c = remove(&mut w, "f2").unwrap();
    assert_eq!(c.removed.unwrap().first, 110);
    let h = current(&mut w);
    assert_eq!(h.count, 2);
    // the 100 sectors where f2 was are free: a file of 100 sectors goes there
    assert_eq!(first_fit_then(&mut w, &h, 100), Some(110));
    let big = unsafe { largest_gap(&IO, &LAY, &h, w.a.0.as_mut_ptr(), 4096, &[]) }.unwrap();
    assert_eq!(big, SECTORS - TAIL - 310);
    // an extent the caller reserves is not given out
    assert_eq!(unsafe { first_fit(&IO, &LAY, &h, w.a.0.as_mut_ptr(), 4096, 100, &[(110, 60)]) }.unwrap(), Some(310));
}

#[test]
fn the_table_goes_to_a_gap_and_never_over_the_one_in_use() {
    let _g = lock();
    fresh();
    let mut w = work();
    let mut at = Vec::new();
    for i in 0..6u32 {
        let c = insert(&mut w, &std::format!("f{i}"), 200 + i * 4, 4 * 512, &[]).unwrap();
        at.push((c.h.table_first, c.h.table_sectors));
    }
    // successive tables are written to different places (the old one stays valid until the headers move)
    for pair in at.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        assert!(a.0 + a.1 <= b.0 || b.0 + b.1 <= a.0, "{a:?} {b:?}");
    }
}

#[test]
fn many_entries_are_streamed_through_small_buffers() {
    let _g = lock();
    fresh();
    let mut w = work();
    // 600 files of one sector from sector 100: a table of 300 sectors, streamed through 1 KiB (4 entries at a time)
    for i in 0..600u32 {
        let name = std::format!("samples/s{i:04}");
        insert(&mut w, &name, 1000 + i, 512, &[]).unwrap_or_else(|e| panic!("entry {i}: error {e}"));
    }
    let h = current(&mut w);
    assert_eq!((h.count, h.table_sectors), (600, 300));
    assert_eq!(h.used, 1 + 300 + 600);
    let p = cstr("samples/s0599");
    let f = unsafe { find(&IO, &LAY, &h, w.a.0.as_mut_ptr(), 4096, p.as_ptr()) }.unwrap().unwrap();
    assert_eq!((f.0, f.1.first), (599, 1599));
    // the table check holds, and the free-space figure counts the room for one more entry
    assert_eq!(unsafe { table_crc(&IO, &LAY, h.table_first, h.table_sectors, w.a.0.as_mut_ptr(), 4096) }.unwrap(), h.table_crc);
    assert_eq!(free_sectors(&LAY, &h), SECTORS - TAIL - h.used - table_sectors(601));
}

#[test]
fn a_clear_leaves_an_empty_store() {
    let _g = lock();
    fresh();
    let mut w = work();
    insert(&mut w, "a", 10, 512, &[]).unwrap();
    insert(&mut w, "b", 11, 512, &[]).unwrap();
    let cur = current(&mut w);
    let c = unsafe { commit(&IO, &LAY, &cur, w.a.0.as_mut_ptr(), w.b.0.as_mut_ptr(), 1024, w.s.0.as_mut_ptr(), Edit::Clear, &[]) }.unwrap();
    assert_eq!((c.h.count, c.h.table_sectors, c.h.used, c.h.seq), (0, 0, 1, 3));
    let t = state(&mut w).unwrap();
    assert_eq!(t.h.count, 0);
    assert!(t.agree);
    assert!(paths(&mut w).is_empty());
}

#[test]
fn a_full_area_refuses_the_table_with_e_full() {
    let _g = lock();
    fresh();
    let mut w = work();
    insert(&mut w, "a", 10, 512, &[]).unwrap();
    // a reserved extent covers every free sector: no gap is left for the next table
    let cur = current(&mut w);
    let big = (1, SECTORS - 3 - 1);
    let c = insert(&mut w, "b", 11, 512, &[big]);
    assert_eq!(c.err(), Some(E_FULL));
    assert_eq!(current(&mut w), cur);
}

/// A power cut after any number of writes of a commit leaves the old store or the new one: both are readable and
/// the data of the old one is intact.
#[test]
fn a_power_cut_at_any_write_leaves_the_old_store_or_the_new_one() {
    let _g = lock();
    for case in 0..4 {
        for cut in 0..40u32 {
            fresh();
            let mut w = work();
            insert(&mut w, "a", 10, 512, &[]).unwrap();
            insert(&mut w, "b", 11, 512, &[]).unwrap();
            let old = paths(&mut w);
            *CUT.lock().unwrap() = (cut, 0);
            let r = match case {
                0 => insert(&mut w, "c", 12, 512, &[]).map(|_| ()),
                1 => remove(&mut w, "a").map(|_| ()),
                3 => rename(&mut w, "a", "d/a").map(|_| ()),
                _ => {
                    let cur = current(&mut w);
                    unsafe { commit(&IO, &LAY, &cur, w.a.0.as_mut_ptr(), w.b.0.as_mut_ptr(), 1024, w.s.0.as_mut_ptr(), Edit::Clear, &[]) }.map(|_| ())
                }
            };
            *CUT.lock().unwrap() = (u32::MAX, 0);
            let now = paths(&mut w);
            let t = state(&mut w).unwrap_or_else(|e| panic!("case {case} cut {cut}: the store is unreadable ({e})"));
            if r.is_ok() {
                assert_ne!(now, old, "case {case} cut {cut}: the commit finished and changed nothing");
            } else {
                // failed: the old store, or (the cut was after header B) the new one
                let new_ok = match case {
                    0 => now.len() == 3,
                    1 => now.len() == 1,
                    3 => now == [("d/a".into(), 10), ("b".into(), 11)],
                    _ => now.is_empty(),
                };
                assert!(now == old || new_ok, "case {case} cut {cut}: {now:?}");
            }
            // whichever it is, the table checks
            let ok = unsafe { table_crc(&IO, &LAY, t.h.table_first, t.h.table_sectors, w.a.0.as_mut_ptr(), 4096) }.unwrap();
            assert_eq!(ok, t.h.table_crc, "case {case} cut {cut}");
        }
    }
}

#[test]
fn header_copies_fall_back_and_the_lagging_one_is_repaired() {
    let _g = lock();
    fresh();
    let mut w = work();
    insert(&mut w, "a", 10, 512, &[]).unwrap();
    insert(&mut w, "b", 11, 512, &[]).unwrap();
    // A torn: B is used
    CARD.lock().unwrap()[30] ^= 1;
    let t = state(&mut w).unwrap();
    assert_eq!((t.h.seq, t.copy, t.agree), (2, 1, false));
    assert!(unsafe { sync_headers(&IO, &LAY, &t, w.s.0.as_mut_ptr()) });
    let t = state(&mut w).unwrap();
    assert_eq!((t.h.seq, t.copy, t.agree), (2, 0, true));
    // B torn (a flipped bit under its CRC): A is used
    CARD.lock().unwrap()[LAY.header_b() as usize * 512 + 9] ^= 1;
    let t = state(&mut w).unwrap();
    assert_eq!((t.h.seq, t.copy, t.agree), (2, 0, false));
    // both torn
    CARD.lock().unwrap()[30] ^= 1;
    assert_eq!(state(&mut w).err(), Some(E_BAD_STORE));
}

#[test]
fn a_higher_seq_in_b_wins_and_a_table_that_does_not_check_is_skipped() {
    let _g = lock();
    fresh();
    let mut w = work();
    insert(&mut w, "a", 10, 512, &[]).unwrap();
    let c = insert(&mut w, "b", 11, 512, &[]).unwrap();
    // header A lags: seq 1 with the old table (as after a cut between B and A)
    let mut old = c.h;
    old.seq = 1;
    unsafe { encode_header(&old, w.s.0.as_mut_ptr()) };
    CARD.lock().unwrap()[..512].copy_from_slice(&w.s.0);
    let t = state(&mut w).unwrap();
    assert_eq!((t.h.seq, t.copy, t.agree), (2, 1, false));
    // B names a table that does not check: A (seq 1, pointing at the same table here) fails too, so the store is bad;
    // with a good table behind A, A is used
    let mut bad = c.h;
    bad.table_crc ^= 1;
    unsafe { encode_header(&bad, w.s.0.as_mut_ptr()) };
    let b_at = LAY.header_b() as usize * 512;
    CARD.lock().unwrap()[b_at..b_at + 512].copy_from_slice(&w.s.0);
    let good_a = Hdr { seq: 1, ..c.h };
    unsafe { encode_header(&good_a, w.s.0.as_mut_ptr()) };
    CARD.lock().unwrap()[..512].copy_from_slice(&w.s.0);
    let t = state(&mut w).unwrap();
    assert_eq!((t.h.seq, t.copy), (1, 0));
}

#[test]
fn a_store_made_for_another_area_is_not_this_one() {
    let _g = lock();
    fresh();
    let mut w = work();
    let c = insert(&mut w, "a", 10, 512, &[]).unwrap();
    let h = Hdr { area: SECTORS + 1, ..c.h };
    unsafe { encode_header(&h, w.s.0.as_mut_ptr()) };
    CARD.lock().unwrap()[..512].copy_from_slice(&w.s.0);
    let b_at = LAY.header_b() as usize * 512;
    CARD.lock().unwrap()[b_at..b_at + 512].copy_from_slice(&w.s.0);
    assert_eq!(state(&mut w).err(), Some(E_BAD_STORE));
}

#[test]
fn paths_and_their_relations() {
    unsafe {
        for (p, ok) in [
            ("a", true),
            ("modules/a.dkm", true),
            ("samples/drums/kick 1.wav", true),
            ("", false),
            ("/a", false),
            ("a//b", false),
            ("a/../b", false),
            ("./a", false),
            ("a/.", false),
            ("a\tb", false),
        ] {
            let c = cstr(p);
            assert_eq!(path_ok(c.as_ptr(), 160).is_some(), ok, "{p:?}");
        }
        let long = cstr(&"x".repeat(159));
        assert_eq!(path_ok(long.as_ptr(), 160), Some(159));
        let too = cstr(&"x".repeat(160));
        assert_eq!(path_ok(too.as_ptr(), 161), None);
        // relation(path, entry)
        let mk = |p: &str, kind: u8| {
            let mut e = [0u8; ENTRY];
            let c = cstr(p);
            encode_entry(c.as_ptr(), &entry(1, 1, kind), e.as_mut_ptr());
            e
        };
        let (file_a, dir_a) = (mk("a", KIND_FILE), mk("a", KIND_DIR));
        let inside = mk("a/b", KIND_FILE);
        let other = mk("ab", KIND_FILE);
        let pa = cstr("a");
        let pab = cstr("a/b");
        assert_eq!(relation(pa.as_ptr(), file_a.as_ptr()), 1);
        assert_eq!(relation(pa.as_ptr(), dir_a.as_ptr()), 1);
        assert_eq!(relation(pa.as_ptr(), inside.as_ptr()), 2);
        assert_eq!(relation(pab.as_ptr(), file_a.as_ptr()), 3);
        assert_eq!(relation(pab.as_ptr(), dir_a.as_ptr()), 0); // a folder is not a file: files may live in it
        assert_eq!(relation(pa.as_ptr(), other.as_ptr()), 0);
    }
}

// ---- a reader against a commit in another task ----

static HOOK: Mutex<Option<(u32, u32)>> = Mutex::new(None); // (reads so far, read number at which the commit runs)

unsafe extern "C" fn rd_hooked(sector: u32, bytes: u32, buf: *mut u8) -> i32 {
    let fire = {
        let mut g = HOOK.lock().unwrap();
        match g.as_mut() {
            Some(h) => {
                let n = h.0;
                h.0 += 1;
                n == h.1
            }
            None => false,
        }
    };
    if fire {
        // a commit in the middle of the read: a new file and its table
        let mut w = work();
        insert(&mut w, "late", 300, 512, &[]).unwrap();
    }
    rd(sector, bytes, buf)
}

#[test]
fn a_commit_during_a_stable_read_is_never_half_seen() {
    let _g = lock();
    let hooked = Io { rd: rd_hooked, wr };
    for at in 0..12u32 {
        fresh();
        let mut w = work();
        insert(&mut w, "a", 10, 512, &[]).unwrap();
        insert(&mut w, "b", 11, 512, &[]).unwrap();
        *HOOK.lock().unwrap() = Some((0, at));
        let mut count = 0;
        let mut buf = work();
        let r = unsafe {
            stable(&hooked, &LAY, buf.s.0.as_mut_ptr(), || {
                let t = open(&hooked, &LAY, buf.b.0.as_mut_ptr().add(0), buf.a.0.as_mut_ptr(), 4096, true)?;
                let mut n = 0;
                let r = scan(&hooked, &LAY, &t.h, buf.a.0.as_mut_ptr(), 4096, |_, _| {
                    n += 1;
                    true
                });
                if r != OK {
                    return Err(r);
                }
                count = n;
                Ok((t.h.count, n))
            })
        };
        *HOOK.lock().unwrap() = None;
        let (hc, n) = r.unwrap_or_else(|e| panic!("read {at}: error {e}"));
        assert_eq!(hc, n, "the header and the table agree (commit at read {at})");
        assert!(hc == 2 || hc == 3, "{hc}");
        assert_eq!(count, n);
    }
}

#[test]
fn a_store_that_reaches_into_the_tail_is_not_used() {
    // A store written before the settings area had a tail of 3 sectors: a file may lie where the settings copies and header B are now.
    let _g = lock();
    fresh();
    let mut w = work();
    let end = LAY.header_b();
    // a file that ends exactly at header B is fine (the allocator's own limit)
    insert(&mut w, "last", end - 4, 4 * 512, &[]).unwrap();
    let t = state(&mut w).unwrap();
    assert!(!unsafe { reaches_tail(&IO, &LAY, &t.h, w.a.0.as_mut_ptr(), 4096) }.unwrap());
    // one sector more, into the settings area: both header copies say so, and there is no usable copy (a reader never takes it as a store)
    insert(&mut w, "late", end - 2, 4 * 512, &[]).unwrap_or_else(|e| panic!("{e}"));
    let h = current(&mut w);
    assert_eq!(h.count, 0, "the store is not read: `current` falls back to the empty store");
    assert_eq!(state(&mut w).err(), Some(E_BAD_STORE));
    // a table in the tail
    fresh();
    let mut w = work();
    insert(&mut w, "a", 10, 512, &[]).unwrap();
    let t = state(&mut w).unwrap();
    let moved = Hdr { table_first: end, ..t.h };
    assert!(unsafe { reaches_tail(&IO, &LAY, &moved, w.a.0.as_mut_ptr(), 4096) }.unwrap());
}

#[test]
fn a_rename_keeps_the_data_and_the_sha256_and_the_table_order() {
    let _g = lock();
    fresh();
    let mut w = work();
    insert(&mut w, "samples/kick", 10, 1000, &[]).unwrap();
    insert(&mut w, "samples/snare", 20, 2000, &[]).unwrap();
    mkdir(&mut w, "aaa").unwrap();
    let before: Vec<_> = entries(&mut w);
    let kick = before.iter().find(|x| x.0 == "samples/kick").unwrap().1;
    let c = rename(&mut w, "samples/kick", "samples/Kick 909").unwrap();
    assert_eq!(c.removed, Some(kick));
    let now = entries(&mut w);
    assert_eq!(now.len(), 3);
    let moved = now.iter().find(|x| x.0 == "samples/Kick 909").unwrap().1;
    assert_eq!(moved, kick, "the same data, size, kind and sha256");
    assert!(!now.iter().any(|x| x.0 == "samples/kick"));
    // sorted by first sector, then path: the folder entry (no data) first, then the files at 10 and 20
    let order: Vec<_> = now.iter().map(|x| x.0.as_str()).collect();
    assert_eq!(order, ["aaa", "samples/Kick 909", "samples/snare"]);
    let h = current(&mut w);
    assert_eq!(h.used, 1 + h.table_sectors + 2 + 4, "the files' sectors are counted once");
    assert!(state(&mut w).is_ok());
}

#[test]
fn a_move_keeps_the_folder_it_leaves_and_drops_the_folder_entry_it_enters() {
    let _g = lock();
    fresh();
    let mut w = work();
    insert(&mut w, "samples/drums/kick", 10, 1000, &[]).unwrap();
    mkdir(&mut w, "samples/loops").unwrap();
    // into an empty folder: its folder entry goes (the folder is implied by the file now), and the folder the file left stays
    rename(&mut w, "samples/drums/kick", "samples/loops/kick").unwrap();
    let now: Vec<_> = entries(&mut w).into_iter().map(|x| (x.0, x.1.kind)).collect();
    assert_eq!(now, [("samples/drums".into(), KIND_DIR), ("samples/loops/kick".into(), KIND_FILE)]);
    // a folder with another file left in it needs no folder entry
    insert(&mut w, "samples/loops/hat", 20, 1000, &[]).unwrap();
    rename(&mut w, "samples/loops/hat", "samples/drums/hat").unwrap();
    let now: Vec<_> = entries(&mut w).into_iter().map(|x| x.0).collect();
    assert_eq!(now, ["samples/loops/kick", "samples/drums/hat"]);
    // to the top: nothing to keep at the top, and the folder left behind stays
    rename(&mut w, "samples/drums/hat", "hat").unwrap();
    let now: Vec<_> = entries(&mut w).into_iter().map(|x| x.0).collect();
    assert_eq!(now, ["samples/drums", "samples/loops/kick", "hat"]);
    assert!(state(&mut w).is_ok());
}

#[test]
fn an_empty_folder_can_be_renamed() {
    let _g = lock();
    fresh();
    let mut w = work();
    mkdir(&mut w, "samples/new").unwrap();
    insert(&mut w, "samples/kick", 10, 1000, &[]).unwrap();
    rename(&mut w, "samples/new", "samples/Field rec").unwrap();
    let now: Vec<_> = entries(&mut w).into_iter().map(|x| (x.0, x.1.kind)).collect();
    assert_eq!(now, [("samples/Field rec".into(), KIND_DIR), ("samples/kick".into(), KIND_FILE)]);
}

#[test]
fn a_rename_that_breaks_a_rule_is_refused_and_changes_nothing() {
    let _g = lock();
    fresh();
    let mut w = work();
    insert(&mut w, "samples/kick", 10, 1000, &[]).unwrap();
    insert(&mut w, "samples/drums/snare", 20, 1000, &[]).unwrap();
    mkdir(&mut w, "samples/empty").unwrap();
    let old = entries(&mut w);
    let seq = current(&mut w).seq;
    for (from, to, want) in [
        ("samples/nope", "samples/x", E_NOFILE),               // no such entry
        ("samples/drums", "samples/x", E_NOFILE),              // a folder with files is not an entry: not moved here
        ("samples/kick", "samples/kick", E_PATH),              // the same path
        ("samples/kick", "samples/drums/snare", E_PATH),       // a file has that path
        ("samples/kick", "samples/drums", E_PATH),             // an implied folder has that path
        ("samples/kick", "samples/empty", E_PATH),             // a folder entry has that path
        ("samples/drums/snare", "samples/kick/snare", E_PATH), // inside a file
        ("samples/kick", "samples/kick/x", E_PATH),            // a file into itself
        ("samples/empty", "samples/empty/in", E_PATH),         // a folder into itself
        ("samples/kick", "samples//x", E_PATH),                // not a store path
        ("samples/kick", "samples/../x", E_PATH),
    ] {
        assert_eq!(rename(&mut w, from, to).err(), Some(want), "{from} -> {to}");
    }
    assert_eq!(entries(&mut w), old);
    assert_eq!(current(&mut w).seq, seq, "no commit was made");
    // a name that only starts like a file is free: "samples/kick2" is not inside "samples/kick"
    rename(&mut w, "samples/drums/snare", "samples/kick2").unwrap();
}
