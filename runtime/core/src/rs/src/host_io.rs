//! Host models of what the store and the DT2 file routes call, for tests of the real store, writer and adapters (`store_tests.rs`):
//!
//! - the firmware's block read and write (the anchors `cf.storage.fn.block.*` of `cleanup_tests.rs` call `card_read` and `card_write`): a sector map
//!   with counters, installed for the length of a test by `CardGuard`. Without a card the anchors panic, as before.
//! - the +Drive write adapter `dkpw_*` (the `#[no_mangle]` functions of `cleanup_tests.rs` call `pdw_*`): absent until `PdwGuard` installs it, then a
//!   file map with a call log.
//! - the firmware file system behind `dkrt_sample_fs` (`FsGuard` installs the functions through the seam `storage_adapter::test_install_fs`).
//!
//! Every guard clears its model on drop. The tests that use them hold `test_lock::LOCK`.
use crate::hash32;
use std::collections::BTreeMap;
use std::string::String;
use std::sync::{Mutex, MutexGuard};
use std::vec::Vec;

fn grab<T>(m: &'static Mutex<T>) -> MutexGuard<'static, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// ---- the card -------------------------------------------------------------------------------------------------------------------

pub(crate) struct HostCard {
    pub sectors: BTreeMap<u32, [u8; 512]>,
    /// A sector never written reads as this byte.
    pub fill: u8,
    pub total: u32,
    pub reads: u32,
    pub writes: u32,
    /// A read that covers one of these sectors fails.
    pub bad_reads: Vec<u32>,
}

static CARD: Mutex<Option<HostCard>> = Mutex::new(None);

/// A card is installed while this lives.
pub(crate) struct CardGuard;

impl CardGuard {
    pub(crate) fn install() -> CardGuard {
        *grab(&CARD) = Some(HostCard { sectors: BTreeMap::new(), fill: 0xff, total: 1 << 20, reads: 0, writes: 0, bad_reads: Vec::new() });
        CardGuard
    }
}

impl Drop for CardGuard {
    fn drop(&mut self) {
        *grab(&CARD) = None;
    }
}

/// The card, for a test that reads its counters or changes a byte (a bit rot, a torn sector).
pub(crate) fn card<R>(f: impl FnOnce(&mut HostCard) -> R) -> R {
    f(grab(&CARD).as_mut().expect("no host card installed"))
}

/// Flip the bits of `mask` in byte `at` of card sector `sector`.
pub(crate) fn card_flip(sector: u32, at: usize, mask: u8) {
    card(|c| {
        let fill = c.fill;
        c.sectors.entry(sector).or_insert([fill; 512])[at] ^= mask;
    })
}

/// The block read anchor: `None` when no card is installed.
pub(crate) fn card_read(sector: u32, bytes: u32, buf: *mut u8) -> Option<i32> {
    let mut g = grab(&CARD);
    let c = g.as_mut()?;
    c.reads += 1;
    let n = bytes / 512;
    if sector as u64 + n as u64 > c.total as u64 || (0..n).any(|i| c.bad_reads.contains(&(sector + i))) {
        return Some(-1);
    }
    for i in 0..n {
        let s = c.sectors.get(&(sector + i)).copied().unwrap_or([c.fill; 512]);
        unsafe { core::ptr::copy_nonoverlapping(s.as_ptr(), buf.add(i as usize * 512), 512) };
    }
    Some(0)
}

/// The capacity getter anchor: the installed card's total, or 0xFFFFFFFF (the driver is not ready) when no card is installed.
pub(crate) fn card_capacity() -> u32 {
    grab(&CARD).as_ref().map_or(u32::MAX, |c| c.total)
}

/// The block write anchor: `None` when no card is installed.
pub(crate) fn card_write(sector: u32, bytes: u32, buf: *const u8) -> Option<i32> {
    let mut g = grab(&CARD);
    let c = g.as_mut()?;
    c.writes += 1;
    let n = bytes / 512;
    if sector as u64 + n as u64 > c.total as u64 {
        return Some(-1);
    }
    for i in 0..n {
        let mut s = [0u8; 512];
        unsafe { core::ptr::copy_nonoverlapping(buf.add(i as usize * 512), s.as_mut_ptr(), 512) };
        c.sectors.insert(sector + i, s);
    }
    Some(0)
}

// ---- the +Drive write adapter ---------------------------------------------------------------------------------------------------

struct Staged {
    path: Vec<u8>,
    size: u32,
    hash4: Option<[u8; 4]>,
    data: Vec<u8>,
}

pub(crate) struct Pdw {
    pub present: bool,
    /// The files the adapter holds, by path as `dkpw_begin` got it (no leading '/').
    pub files: BTreeMap<Vec<u8>, Vec<u8>>,
    /// The calls, in order: `begin <path> <size> <flags>`, `write <len>`, `close`, `abort`, `delete <path>`, `recover`, `commit`.
    pub calls: Vec<String>,
    staged: Option<Staged>,
}

static PDW: Mutex<Pdw> = Mutex::new(Pdw { present: false, files: BTreeMap::new(), calls: Vec::new(), staged: None });

pub(crate) fn pdw<R>(f: impl FnOnce(&mut Pdw) -> R) -> R {
    f(&mut grab(&PDW))
}

/// The adapter is in the image while this lives.
pub(crate) struct PdwGuard;

impl PdwGuard {
    pub(crate) fn install() -> PdwGuard {
        pdw(|p| {
            *p = Pdw { present: true, files: BTreeMap::new(), calls: Vec::new(), staged: None };
        });
        crate::platform::test_forget_staged_writer();
        PdwGuard
    }
}

impl Drop for PdwGuard {
    fn drop(&mut self) {
        pdw(|p| *p = Pdw { present: false, files: BTreeMap::new(), calls: Vec::new(), staged: None });
        crate::platform::test_forget_staged_writer();
    }
}

/// The absent adapter's answers (what the host fixture gave before the fake existed): E_CONTRACT.
const ABSENT: i32 = -48;

unsafe fn cstr(p: *const u8) -> Vec<u8> {
    let mut v = Vec::new();
    let mut i = 0;
    while *p.add(i) != 0 {
        v.push(*p.add(i));
        i += 1;
    }
    v
}

pub(crate) fn pdw_version() -> u32 {
    if pdw(|p| p.present) {
        crate::platform::DKPW_VERSION
    } else {
        0
    }
}

pub(crate) fn pdw_begin(path: *const u8, size: u32, hash4: *const u8, flags: u32) -> i32 {
    unsafe {
        let mut g = grab(&PDW);
        if !g.present {
            return ABSENT;
        }
        let path = cstr(path);
        g.calls.push(std::format!("begin {} {} {}", String::from_utf8_lossy(&path), size, flags));
        let hash4 = if hash4.is_null() { None } else { Some([*hash4, *hash4.add(1), *hash4.add(2), *hash4.add(3)]) };
        g.staged = Some(Staged { path, size, hash4, data: Vec::new() });
        0
    }
}

pub(crate) fn pdw_write(buf: *const u8, len: u32) -> i32 {
    let mut g = grab(&PDW);
    if !g.present {
        return ABSENT;
    }
    g.calls.push(std::format!("write {len}"));
    match g.staged.as_mut() {
        Some(s) => {
            s.data.extend_from_slice(unsafe { core::slice::from_raw_parts(buf, len as usize) });
            0
        }
        None => dkrt_sdk::proto::E_STATE,
    }
}

pub(crate) fn pdw_close(out: *mut u8) -> i32 {
    let mut g = grab(&PDW);
    if !g.present {
        return ABSENT;
    }
    g.calls.push(String::from("close"));
    let Some(s) = g.staged.take() else { return dkrt_sdk::proto::E_STATE };
    if s.data.len() as u32 != s.size {
        return dkrt_sdk::proto::E_LENGTH;
    }
    let slot = unsafe { hash32::slot_of(s.data.as_ptr(), s.data.len() as u32) };
    if !out.is_null() {
        unsafe { core::ptr::copy_nonoverlapping(slot.as_ptr(), out, 4) };
    }
    if s.hash4.is_some_and(|h| h != slot[..4]) {
        return dkrt_sdk::proto::E_HASH;
    }
    g.files.insert(s.path, s.data);
    0
}

pub(crate) fn pdw_abort() {
    let mut g = grab(&PDW);
    if g.present {
        g.calls.push(String::from("abort"));
        g.staged = None;
    }
}

pub(crate) fn pdw_delete(path: *const u8) -> i32 {
    unsafe {
        let mut g = grab(&PDW);
        if !g.present {
            return ABSENT;
        }
        let path = cstr(path);
        g.calls.push(std::format!("delete {}", String::from_utf8_lossy(&path)));
        if g.files.remove(&path).is_some() {
            0
        } else {
            dkrt_sdk::proto::E_NOFILE
        }
    }
}

pub(crate) fn pdw_recover(out: *mut u32) -> i32 {
    let mut g = grab(&PDW);
    if !g.present {
        return ABSENT;
    }
    g.calls.push(String::from("recover"));
    unsafe { core::ptr::write_bytes(out, 0, 4) };
    0
}

pub(crate) fn pdw_commit() -> i32 {
    let mut g = grab(&PDW);
    if g.present {
        g.calls.push(String::from("commit"));
    }
    0
}

// ---- the firmware file system (DT2 samples) --------------------------------------------------------------------------------------------

pub(crate) struct HostFs {
    /// The files by the path the adapter opens: with the leading '/'.
    pub files: BTreeMap<Vec<u8>, Vec<u8>>,
    /// Every path opened, in order.
    pub opens: Vec<Vec<u8>>,
    handles: Vec<(Vec<u8>, usize)>,
}

static FS: Mutex<HostFs> = Mutex::new(HostFs { files: BTreeMap::new(), opens: Vec::new(), handles: Vec::new() });

pub(crate) fn fs<R>(f: impl FnOnce(&mut HostFs) -> R) -> R {
    f(&mut grab(&FS))
}

/// The odd hash word the model's metadata call reports for a file of `len` bytes.
pub(crate) fn fs_odd_hash(len: usize) -> u32 {
    (len as u32).wrapping_mul(7).wrapping_add(1) | 1
}

unsafe extern "C" fn fs_open(path: *const u8, _mode: *const u8, handle: *mut u32) -> i32 {
    let path = cstr(path);
    let mut g = grab(&FS);
    g.opens.push(path.clone());
    if !g.files.contains_key(&path) {
        return -1;
    }
    g.handles.push((path, 0));
    *handle = g.handles.len() as u32;
    0
}

unsafe extern "C" fn fs_read(dst: *mut u8, n: u32, handle: *mut u32) -> i32 {
    let mut g = grab(&FS);
    let id = *handle as usize;
    let (path, pos) = g.handles[id - 1].clone();
    let data = &g.files[&path];
    let take = (n as usize).min(data.len() - pos);
    core::ptr::copy_nonoverlapping(data.as_ptr().add(pos), dst, take);
    g.handles[id - 1].1 = pos + take;
    take as i32
}

unsafe extern "C" fn fs_close(_: *mut u32) -> i32 {
    0
}

unsafe extern "C" fn fs_size(id: u32) -> u32 {
    let g = grab(&FS);
    g.files[&g.handles[id as usize - 1].0].len() as u32
}

unsafe extern "C" fn fs_meta(id: u32, values: *mut u32) -> i32 {
    let g = grab(&FS);
    let len = g.files[&g.handles[id as usize - 1].0].len();
    *values = id;
    *values.add(1) = fs_odd_hash(len);
    *values.add(2) = len as u32;
    *values.add(3) = 0;
    0
}

/// The firmware file system is behind `dkrt_sample_fs` while this lives; `files` are keyed by their rooted path (`/KICKS/K1`).
pub(crate) struct FsGuard;

impl FsGuard {
    pub(crate) fn install(files: &[(&str, &[u8])]) -> FsGuard {
        fs(|f| {
            f.files = files.iter().map(|(p, d)| (p.as_bytes().to_vec(), d.to_vec())).collect();
            f.opens.clear();
            f.handles.clear();
        });
        crate::storage::backends::fwfs::test_install_fs(fs_open, fs_read, fs_close, fs_size, Some(fs_meta));
        FsGuard
    }
}

impl Drop for FsGuard {
    fn drop(&mut self) {
        crate::storage::backends::fwfs::test_remove_fs();
        fs(|f| {
            f.files.clear();
            f.opens.clear();
            f.handles.clear();
        });
    }
}
