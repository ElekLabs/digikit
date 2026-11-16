//! The two routes behind `Storage for RealPlatform` (platform.rs calls only this file): the digikit store (`ROOT_STORE` on both devices, and `ROOT_SAMPLES`
//! as the `samples/` prefix of the store when the device keeps its samples there) and the DT2 firmware file system (`ROOT_SAMPLES` otherwise).
//! This file names no firmware crate: the file system is reached through the device tables `dkrt_sample_fs` and `dkrt_files_fs` of the runtime's
//! assembly objects (zeros on a device without them).
//!
//! Every call runs in task context. Device facts come from `Facts` (the profile), never from a device id.
//!
//! A listing keeps all its state in a block the caller provides (`list_state_bytes`): `Dt2List` or `StoreList`, told apart by the tag in the first word.

use core::ptr::addr_of_mut;
use dkrt_abi::storage::{DirEntry, EntryKind, ListStep, Stat, HASH_ELEKTRON32, HASH_NONE, ROOT_SAMPLES};
use dkrt_sdk::store::catalog::{CatalogCursor, CatalogEntry, CatalogOpen};
use dkrt_sdk::store::directory::DirectoryCollector;
use dkrt_sdk::store::PATH_BYTES;
use super::backends::fwfs;
use super::route::{prepare, store_key, Backend, Facts};
use dkrt_sdk::{E_BAD_STORE, E_BUSY, E_FORMAT, E_NOT_FOUND, E_SEL_PATH, OK};

fn dir_stat() -> Stat {
    Stat { kind: EntryKind::Dir, size: 0, hash_kind: HASH_NONE, hash_valid: false, hash: [0; 32] }
}

// ---- store route -------------------------------------------------------------------------------------------------------------

#[repr(C, align(16))]
struct Scratch([u8; 4096]);
#[repr(C, align(16))]
struct Fingerprint([u8; 512]);
static mut SCRATCH: Scratch = Scratch([0; 4096]);
static mut FP: Fingerprint = Fingerprint([0; 512]);
const RAW: CatalogEntry = CatalogEntry { path: [0; 160], entry: dkrt_sdk::store::Entry { first: 0, size: 0, kind: 0, sha: [0; 32] } };
static mut RAW_PAGE: [CatalogEntry; 16] = [RAW; 16];
/// The store buffers of a catalog read (fingerprint, 4096-byte scratch, raw page). They are shared by all listings, not part of a listing's state: callers
/// hold the store lock for every step that uses them (`list_next` does).
fn store_buffers() -> (*mut u8, *mut u8, *mut [CatalogEntry; 16]) {
    unsafe { (addr_of_mut!(FP) as *mut u8, addr_of_mut!(SCRATCH) as *mut u8, addr_of_mut!(RAW_PAGE)) }
}

// ---- the Storage calls -------------------------------------------------------------------------------------------------------

/// Kind, size and hash. The store gives the table entry's size and file hash (kind `HASH_ELEKTRON32`, valid, the 32 bytes), as `store_identity`
/// (`sample_tracks.rs` of the SDK) does for `dk_file_id` and as the store listing rows do (files.rs, phase 3). The firmware file system gives the size of
/// the open file and, when the metadata call answers with a physical size equal to it, the odd hash as `HASH_ELEKTRON32` (4 bytes): the rule of
/// tracks.rs `probe`. A folder is the root itself (empty path) only: the store's lookup finds files, and a folder does not open as a file on DT2.
pub(crate) unsafe fn stat(root: u32, f: &Facts, path: &[u8]) -> Result<Stat, i32> {
    let r = prepare(root, f, path)?;
    if path.is_empty() {
        return Ok(dir_stat());
    }
    if r == Backend::Store {
        let mut z = [0u8; PATH_BYTES];
        store_key(root, path, &mut z)?;
        let file = crate::store::find(z.as_ptr())?;
        return Ok(Stat { kind: EntryKind::File, size: file.size, hash_kind: HASH_ELEKTRON32, hash_valid: true, hash: file.sha });
    }
    fwfs::stat(path)
}

/// `stat` and the head of the file with one lookup: the store route does one `store::find` and reads the head of that entry; the DT2 route opens the file once, takes
/// its size and metadata and reads 64 bytes. The root itself (empty path) gives its `Stat` and 0 head bytes; a folder does not open on either route. The head
/// count of a success is 64 (a file under 64 bytes is `E_READ`, as in `store::read_head`).
pub(crate) unsafe fn stat_head(root: u32, f: &Facts, path: &[u8], out: &mut [u8; 64]) -> Result<(Stat, usize), i32> {
    let r = prepare(root, f, path)?;
    if path.is_empty() {
        return Ok((dir_stat(), 0));
    }
    if r == Backend::Store {
        let mut z = [0u8; PATH_BYTES];
        store_key(root, path, &mut z)?;
        let file = crate::store::find(z.as_ptr())?;
        let st = Stat { kind: EntryKind::File, size: file.size, hash_kind: HASH_ELEKTRON32, hash_valid: true, hash: file.sha };
        crate::store::read_head(&file, out)?;
        return Ok((st, 64));
    }
    fwfs::stat_head(path, out)
}

/// Read the whole file into `dst` (`cap` bytes); the size. A file over `cap` is `E_TOO_BIG` and nothing is written. Store: `store::read_file` (checks the
/// table's file hash). DT2: size from the open file, then sequential reads.
pub(crate) unsafe fn read_all(root: u32, f: &Facts, path: &[u8], dst: *mut u8, cap: u32) -> Result<u32, i32> {
    let r = prepare(root, f, path)?;
    if path.is_empty() {
        return Err(E_SEL_PATH);
    }
    if r == Backend::Store {
        let mut z = [0u8; PATH_BYTES];
        store_key(root, path, &mut z)?;
        let n = crate::store::read_file(z.as_ptr(), dst, cap);
        return if n < 0 { Err(n) } else { Ok(n as u32) };
    }
    fwfs::read_all(path, dst, cap)
}

// ---- listing ---------------------------------------------------------------------------------------------------------------

/// Tags of the first word of the state block (the header the caller must not touch).
const TAG_STORE: u32 = 0x5354_0001;

/// Store listing state: the phases of files.rs `Backend` (1 open the catalog, 2 read a page, 3 give the page rows, 4 end), without the DT2 and queue fields.
#[repr(C)]
struct StoreList {
    tag: u32,
    phase: u8,
    virtual_root: bool,
    more: bool,
    directory_len: usize,
    index: usize,
    directory: [u8; 160],
    after: [u8; 160],
    opener: Option<CatalogOpen>,
    cursor: Option<CatalogCursor>,
    collector: Option<DirectoryCollector<8>>,
}

/// Bytes of listing state: the larger of the two route structs, a multiple of 16.
pub(crate) fn list_state_bytes() -> usize {
    let n = core::cmp::max(core::mem::size_of::<fwfs::Dt2List>(), core::mem::size_of::<StoreList>());
    (n + 15) & !15
}

/// Start listing `dir` into `state` (at least `list_state_bytes()` bytes, 16-byte aligned). The tag is written last, so a failed open leaves no valid state.
pub(crate) unsafe fn list_open(root: u32, f: &Facts, dir: &[u8], state: *mut u8) -> Result<(), i32> {
    let r = prepare(root, f, dir)?;
    if r == Backend::Store {
        return store_open(root, dir, state as *mut StoreList);
    }
    fwfs::list_open(dir, state)
}

/// Start a store listing in place (no `StoreList` value is built on the stack; the collector is built by `DirectoryCollector::new` as in files.rs). The
/// directory is the store key of `dir` (`samples/` in front for `ROOT_SAMPLES`).
unsafe fn store_open(root: u32, dir: &[u8], l: *mut StoreList) -> Result<(), i32> {
    let mut key = [0u8; PATH_BYTES];
    let n = store_key(root, dir, &mut key)?;
    addr_of_mut!((*l).tag).write(0);
    addr_of_mut!((*l).phase).write(0);
    addr_of_mut!((*l).virtual_root).write(root == ROOT_SAMPLES && dir.is_empty());
    addr_of_mut!((*l).more).write(false);
    addr_of_mut!((*l).directory_len).write(n);
    addr_of_mut!((*l).index).write(0);
    addr_of_mut!((*l).directory).write([0; 160]);
    addr_of_mut!((*l).after).write([0; 160]);
    addr_of_mut!((*l).opener).write(None);
    addr_of_mut!((*l).cursor).write(None);
    addr_of_mut!((*l).collector).write(None);
    let l = &mut *l;
    l.directory[..n].copy_from_slice(&key[..n]);
    l.collector = Some(DirectoryCollector::new(&l.directory[..n], None)?);
    l.opener = Some(CatalogOpen::begin(&crate::store::reader(), &crate::store::layout())?);
    l.phase = 1;
    l.tag = TAG_STORE;
    Ok(())
}

/// One provider step. DT2: one firmware `next` call; `.` and `..` give `Pending` (as files.rs phase 11); a file's
/// size and hash come from the metadata call by entry id and are set only when it answers (as the old phase 12: no size check against a physical size
/// here, a failed call leaves size 0 and no hash).
pub(crate) unsafe fn list_next(state: *mut u8, out: &mut DirEntry) -> Result<ListStep, i32> {
    let tag = *(state as *const u32);
    if tag == TAG_STORE {
        if !crate::store_writer::try_lock() {
            return Err(E_BUSY);
        }
        let r = store_next(&mut *(state as *mut StoreList), out);
        crate::store_writer::unlock();
        return r;
    }
    if tag != fwfs::TAG_FS {
        return Err(E_FORMAT);
    }
    fwfs::list_next(state, out)
}

/// One store phase of files.rs `Backend::step_inner`, run under the store lock (taken by the caller): open the catalog, read a page and feed the
/// collector, finish, give the page rows one per call, rewind and continue after the last path while `more`. A phase that gives no row returns `Pending`.
/// A virtual `samples` root that does not exist in the store is an empty listing.
unsafe fn store_next(l: &mut StoreList, out: &mut DirEntry) -> Result<ListStep, i32> {
    let (fp, scratch, raw) = store_buffers();
    {
        match l.phase {
            1 => {
                let complete = match l.opener.as_mut().unwrap().step(fp, scratch, 4096) {
                    Err(error) if error == E_NOT_FOUND && l.virtual_root => {
                        l.phase = 4;
                        return Ok(ListStep::End);
                    }
                    result => result?,
                };
                if complete {
                    l.cursor = Some(l.opener.take().unwrap().take()?);
                    l.phase = 2;
                }
                Ok(ListStep::Pending)
            }
            2 => {
                let n = l.cursor.as_mut().unwrap().read_page(fp, scratch, 4096, &mut *raw)?;
                if n != 0 {
                    l.collector.as_mut().unwrap().feed(&(&*raw)[..n])?;
                } else {
                    if let Err(error) = l.collector.as_mut().unwrap().finish(Ok(())) {
                        if error == E_NOT_FOUND && l.virtual_root {
                            l.phase = 4;
                            return Ok(ListStep::End);
                        }
                        return Err(error);
                    }
                    let page = l.collector.as_ref().unwrap().page()?;
                    l.more = page.more;
                    if let Some(last) = page.entries.last() {
                        l.after = last.path;
                    }
                    l.index = 0;
                    l.phase = 3;
                }
                Ok(ListStep::Pending)
            }
            3 => {
                let page = l.collector.as_ref().unwrap().page()?;
                if l.index < page.entries.len() {
                    let value = page.entries[l.index];
                    l.index += 1;
                    let n = dkrt_sdk::store::path_len(value.path.as_ptr());
                    let start = if l.directory_len == 0 { 0 } else { l.directory_len + 1 };
                    if n <= start || n - start > 255 {
                        return Err(E_BAD_STORE);
                    }
                    *out = DirEntry::empty();
                    out.name[..n - start].copy_from_slice(&value.path[start..n]);
                    out.name_len = (n - start) as u16;
                    if value.entry.kind == dkrt_sdk::store::KIND_DIR {
                        out.stat = dir_stat();
                    } else {
                        out.stat = Stat {
                            kind: EntryKind::File,
                            size: value.entry.size,
                            hash_kind: HASH_ELEKTRON32,
                            hash_valid: true,
                            hash: value.entry.sha,
                        };
                    }
                    return Ok(ListStep::Entry);
                }
                if !l.more {
                    l.phase = 4;
                    return Ok(ListStep::End);
                }
                l.cursor.as_mut().unwrap().rewind(fp)?;
                let n = dkrt_sdk::store::path_len(l.after.as_ptr());
                l.collector = Some(DirectoryCollector::new(&l.directory[..l.directory_len], Some(&l.after[..n]))?);
                l.phase = 2;
                Ok(ListStep::Pending)
            }
            4 => Ok(ListStep::End),
            _ => Err(E_FORMAT),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_state_covers_both_routes_in_16_byte_units() {
        let n = list_state_bytes();
        assert!(n >= core::mem::size_of::<fwfs::Dt2List>());
        assert!(n >= core::mem::size_of::<StoreList>());
        assert_eq!(n % 16, 0);
        assert!(core::mem::align_of::<fwfs::Dt2List>() <= 16 && core::mem::align_of::<StoreList>() <= 16);
    }
}
