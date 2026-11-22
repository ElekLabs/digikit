//! The runtime's use of the module and sample store: the format, the reader and the writer are `dkrt_sdk::store` (docs/design/digikit-store.md); this
//! file adds the area of this device, the staging buffer, the file's hash check, `dk_files_read` and the list the DIGIKIT menu shows. Reads go
//! through the firmware's block read (16-byte aligned buffers, multiples of 512 bytes, at most 0x10000 per call). The table is never held whole: it is
//! scanned through the staging buffer.

use crate::hash32;
use dkrt_sdk::STAGE_BYTES;
use crate::{state::DKRT_STAGE};
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use dkrt_sdk::store::{self, Hdr, Io, Layout, Table, KIND_FILE};
use dkrt_sdk::*;

pub use super::layout::layout;
#[cfg(test)]
pub(crate) use super::layout::TEST_LAYOUT;

/// 512 bytes, 16-byte aligned: the header sectors (UI task or the mailbox; the upload has its own buffer).
#[repr(C, align(16))]
pub struct PeekBuf([u8; 512]);
#[no_mangle]
pub static mut DKRT_STORE_PEEK: PeekBuf = PeekBuf([0; 512]);
/// The fingerprint reads of a stable list.
#[no_mangle]
pub static mut DKRT_STORE_FP: PeekBuf = PeekBuf([0; 512]);

pub unsafe fn stage() -> *mut u8 {
    addr_of_mut!(DKRT_STAGE) as *mut u8
}

unsafe fn scratch() -> *mut u8 {
    addr_of_mut!(DKRT_STORE_PEEK) as *mut u8
}

unsafe extern "C" fn block_read(sector: u32, bytes: u32, buf: *mut u8) -> i32 {
    crate::platform::block_read(sector, bytes, buf)
}

unsafe extern "C" fn no_write(_sector: u32, _bytes: u32, _buf: *mut u8) -> i32 {
    -1
}

/// The card for reads (it cannot write).
pub fn reader() -> Io {
    Io { rd: block_read, wr: no_write }
}

/// Where a file is and what its table entry says its file hash slot is.
pub struct File {
    pub first: u32,
    pub size: u32,
    pub sha: [u8; 32],
}

/// The table the reads last checked: (seq, table CRC, table first sector). A commit of the runtime's own upload sets it; a read of a header that
/// differs checks the table's CRC again (one pass over the table), so a lookup costs the headers and one scan.
static mut CHECKED: Option<(u32, u32, u32)> = None;

/// Host tests: a new card in the host model (or a changed one) has no table checked yet.
#[cfg(test)]
pub(crate) fn test_forget_checked() {
    unsafe { write_volatile(addr_of_mut!(CHECKED), None) }
}

/// The upload committed `h`: its table is the one just written and read back.
pub fn note_commit(h: &Hdr) {
    unsafe { write_volatile(addr_of_mut!(CHECKED), Some((h.seq, h.table_crc, h.table_first))) }
}

/// Open the store for a read through the buffers given (`scratch`: 512 bytes, `buf`: `cap` bytes, both 16-byte aligned, apart): the headers, and
/// the table's CRC when the headers are not the ones last checked. `Err(E_NOT_FOUND)` also when the card is smaller than the area (there is no
/// store).
pub unsafe fn open_in(scratch: *mut u8, buf: *mut u8, cap: usize) -> Result<Table, i32> {
    if crate::card::state() == CARD_TOO_SMALL {
        return Err(E_NOT_FOUND);
    }
    let (io, lay) = (reader(), layout());
    let t = store::open(&io, &lay, scratch, buf, cap, false)?;
    let key = (t.h.seq, t.h.table_crc, t.h.table_first);
    if read_volatile(addr_of!(CHECKED)) == Some(key) {
        return Ok(t);
    }
    let t = store::open(&io, &lay, scratch, buf, cap, true)?;
    write_volatile(addr_of_mut!(CHECKED), Some((t.h.seq, t.h.table_crc, t.h.table_first)));
    Ok(t)
}

/// The one rule of "store, empty store or no store" that the INFO and upload steps, the module list and the HEALTH figures all use: `open_in`, and a
/// card that is ours (or blank) with no store header is an empty store (`seq` 0) when both header sectors are blank (or hold a store of another version): a fresh card is
/// ours after the boot's selection record, and the first upload writes the headers. Bytes in a header sector that are not a store header are
/// somebody's data (finding 46): no store. A card too small for the area has no store. Other errors are `open_in`'s (`E_BAD_STORE`, `E_READ`).
pub unsafe fn open_or_empty_in(scratch: *mut u8, buf: *mut u8, cap: usize) -> Result<Table, i32> {
    match open_in(scratch, buf, cap) {
        Err(e) => match headerless_empty(e, scratch) {
            Some(h) => Ok(Table { h, copy: 0, agree: true }),
            None => Err(e),
        },
        r => r,
    }
}

/// The header of the empty store, when the read of the headers failed with `e` for lack of a store header and the card may hold an empty store
/// (`empty_possible`, both header sectors blank or of another version): the one place of that decision, for `open_or_empty_in` and `seen`.
unsafe fn headerless_empty(e: i32, scratch: *mut u8) -> Option<Hdr> {
    if e == E_NOT_FOUND && empty_possible() && headers_blank(scratch) {
        Some(Hdr::empty(&layout()))
    } else {
        None
    }
}

/// May a card without a store header hold an empty store? Only when the runtime may write the card (ours, blank, or not decided yet): a foreign,
/// refused or too small card has none.
fn empty_possible() -> bool {
    crate::card::writable()
}

/// Does each header sector (A, B) read all 0x00 or all 0xFF, or hold a store header of another version? A sector that cannot be read is not free.
unsafe fn headers_blank(scratch: *mut u8) -> bool {
    let l = layout();
    let mut k = 0;
    while k < 2 {
        let rel = if k == 0 { 0 } else { l.header_b() };
        if block_read(l.first + rel, 512, scratch) != 0 || !(crate::card::blank(scratch) || store::old_header(scratch)) {
            return false;
        }
        k += 1;
    }
    true
}

/// `open_in` with the runtime's own buffers (the staging buffer and the peek sector).
pub unsafe fn open() -> Result<Table, i32> {
    open_in(scratch(), stage(), STAGE_BYTES)
}

/// Look `path` up. Leaves garbage in the staging buffer (the table is scanned through it); the result is by value.
pub unsafe fn find(path: *const u8) -> Result<File, i32> {
    if !super::lock::acquire() {
        return Err(E_BUSY);
    }
    let r = find_unlocked(path);
    super::lock::unlock();
    r
}

/// `find` for a caller that holds the store lock.
unsafe fn find_unlocked(path: *const u8) -> Result<File, i32> {
    let t = open()?;
    let (io, lay) = (reader(), layout());
    match store::find(&io, &lay, &t.h, stage(), STAGE_BYTES, path)? {
        Some((_, e)) if e.kind == KIND_FILE => Ok(File { first: e.first, size: e.size, sha: e.sha }),
        _ => Err(E_NOT_FOUND),
    }
}

/// The first 64 bytes of the file `f` (a native sample header), through the peek sector. A file under 64 bytes has no head: `E_READ` (the sector holds
/// other bytes after the end of the file, which are not the file's). Task context, like every store read.
pub unsafe fn read_head(f: &File, head: &mut [u8; 64]) -> Result<(), i32> {
    if f.size < 64 {
        return Err(E_READ);
    }
    let r = read_sector(f.first, scratch());
    if r != OK {
        return Err(r);
    }
    let mut i = 0;
    while i < 64 {
        head[i] = *scratch().add(i);
        i += 1;
    }
    Ok(())
}

/// Read the file `f` into the staging buffer and check the table's file hash. The file is then at `stage()`.
pub unsafe fn load_to_stage(f: &File) -> i32 {
    let r = capacity::stage_fit(f.size, STAGE_BYTES as u32);
    if r != OK {
        return r;
    }
    let sectors = (f.size + 511) / 512;
    let buf = stage();
    // `first` is relative to the start of the store
    if !super::lock::acquire() {
        return E_BUSY;
    }
    let r = store::read_span(&reader(), &layout(), f.first, sectors, buf, STAGE_BYTES);
    super::lock::unlock();
    if r != OK {
        return r;
    }
    if hash32::slot_of(buf, f.size) != f.sha {
        return E_SHA;
    }
    OK
}

/// `dk_files_read`: copy a store file to `dst` (at most `cap` bytes); returns its size or a negative code.
pub unsafe fn read_file(path: *const u8, dst: *mut u8, cap: u32) -> i32 {
    let f = match find(path) {
        Ok(f) => f,
        Err(e) => return e,
    };
    let r = capacity::read_fit(f.size, cap);
    if r != OK {
        return r;
    }
    let r = load_to_stage(&f);
    if r != OK {
        return r;
    }
    let src = stage();
    let mut i = 0;
    while i < f.size as usize {
        *dst.add(i) = *src.add(i);
        i += 1;
    }
    f.size as i32
}

/// The store figures as the headers last said them, and whether the headers may have changed since. Only an upload writes a header sector
/// (the card is not swapped while the device runs), so the HEALTH page, which asks twice a second, reads the card once and again after an
/// upload wrote a header (`store_writer.rs` calls `figures_stale` when it writes one): a page that read the card on every refresh would cost the UI task
/// card reads that show up in the CPU load it displays.
static mut STORE_DIRTY: bool = true;
static mut STORE_SEEN_CARD: u32 = u32::MAX; // the card state of the figures below
static mut STORE_SEEN: Option<(u32, u32)> = None; // (used KiB, free KiB) of a usable header; None: no store

/// An upload wrote (or tried to write) a header sector: the figures are stale. Called from the MIDI task.
pub fn figures_stale() {
    unsafe { core::ptr::write_volatile(addr_of_mut!(STORE_DIRTY), true) }
}

/// Fill the store figures of a `DkrtHealth` from the headers alone (two sectors, no table, and only when an upload changed them), in KiB: the sectors
/// a store may use in all, the sectors used, and what a new file may still take. Sets `HEALTH_STORE` when a usable header was read; a card with no
/// store leaves them 0.
pub unsafe fn figures(o: &mut DkrtHealth) {
    o.store_total = layout().limit() / 2;
    o.store_used = 0;
    o.store_free = 0;
    if let Some((used, free)) = seen() {
        o.store_used = used;
        o.store_free = free;
        o.flags |= HEALTH_STORE;
    }
}

/// (used KiB, free KiB) of the store from the headers (read again only after an upload wrote one), or `None`: no store (a card smaller than the area,
/// or no usable header).
pub unsafe fn seen() -> Option<(u32, u32)> {
    let lay = layout();
    if crate::card::state() == CARD_TOO_SMALL {
        return None;
    }
    // the figures belong to the card state they were read under: a state that changed since (the card check decided later) reads the headers again
    let card_now = crate::card::state();
    if core::ptr::read_volatile(addr_of!(STORE_DIRTY)) || card_now != STORE_SEEN_CARD {
        // an SDK write or a SysEx command is inside the store: keep the last figures (unless they were read under another card state), the flag stays set
        // for the next refresh
        if !super::lock::try_lock() {
            return if card_now == STORE_SEEN_CARD { STORE_SEEN } else { None };
        }
        STORE_SEEN_CARD = card_now;
        // clear first: a header written while we read leaves the flag set for the next refresh
        core::ptr::write_volatile(addr_of_mut!(STORE_DIRTY), false);
        STORE_SEEN = match store::open_headers(&reader(), &lay, scratch()) {
            Ok((a, b)) => {
                let pick = if store::prefer_b(a.as_ref(), b.as_ref()) { b } else { a };
                pick.map(|h| (h.used / 2, store::free_sectors(&lay, &h) / 2))
            }
            Err(e) => headerless_empty(e, scratch()).map(|h| (h.used / 2, store::free_sectors(&lay, &h) / 2)),
        };
        super::lock::unlock();
    }
    STORE_SEEN
}

/// What a listing of a prefix includes: `All` is every table entry (folders, empty files, paths up to the store's 159 bytes: the SysEx LIST, which a host
/// tool shows whole); `Modules` is what the loader and the module scan want (files with data, a path that fits the 64 bytes of `DkrtStoreEntry`).
/// `Storage::list` (`storage_adapter`) is the directory view of the same table and keeps its resumable cursor.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Modules,
}

/// Does the table entry `e` (its path first) belong to the listing of `prefix` (`plen` bytes, no NUL) under `filter`? The one rule of the listers.
pub unsafe fn listed(e: *const u8, prefix: *const u8, plen: usize, filter: Filter) -> bool {
    let len = store::path_len(e);
    if len < plen {
        return false;
    }
    if filter == Filter::Modules {
        let en = store::entry_of(e);
        if en.kind != KIND_FILE || en.size == 0 || len >= 64 {
            return false;
        }
    }
    let mut i = 0;
    while i < plen {
        if *e.add(i) != *prefix.add(i) {
            return false;
        }
        i += 1;
    }
    true
}

/// `list(prefix, out, max)`: the files whose path starts with `prefix` (NUL-terminated, e.g. `modules/`), in path order, at most `max`
/// into `out`; returns how many, or a negative code (`E_NOT_FOUND`: no store). A path that does not fit 63 bytes, a folder and an empty file are
/// left out. A commit of an upload running at the same time makes the list start again (it is one header's table).
pub unsafe fn list(prefix: *const u8, out: *mut DkrtStoreEntry, max: u32) -> i32 {
    if !super::lock::acquire() {
        return E_BUSY;
    }
    let r = list_unlocked(prefix, out, max);
    super::lock::unlock();
    r
}

/// `list` for a caller that holds the store lock.
unsafe fn list_unlocked(prefix: *const u8, out: *mut DkrtStoreEntry, max: u32) -> i32 {
    let (io, lay) = (reader(), layout());
    let fp = addr_of_mut!(DKRT_STORE_FP) as *mut u8;
    let mut plen = 0;
    while *prefix.add(plen) != 0 {
        plen += 1;
    }
    let r = store::stable(&io, &lay, fp, || {
        let t = open_or_empty_in(scratch(), stage(), STAGE_BYTES)?;
        let mut n = 0u32;
        let r = store::scan(&io, &lay, &t.h, stage(), STAGE_BYTES, |_, e| {
            if !listed(e, prefix, plen, Filter::Modules) {
                return true;
            }
            let en = store::entry_of(e);
            let len = store::path_len(e);
            if n >= max {
                return false;
            }
            let o = &mut *out.add(n as usize);
            let mut k = 0;
            while k < 64 {
                o.path[k] = if k < len { *e.add(k) } else { 0 };
                k += 1;
            }
            o.first = en.first;
            o.size = en.size;
            n += 1;
            true
        });
        if r != OK {
            return Err(r);
        }
        Ok(n)
    });
    let n = match r {
        Ok(n) => n,
        Err(e) => return e,
    };
    // path order (insertion sort: the table is in position order)
    let mut i = 1;
    while i < n as usize {
        let mut j = i;
        while j > 0 && store::cstr_cmp((*out.add(j - 1)).path.as_ptr(), (*out.add(j)).path.as_ptr()) > 0 {
            core::ptr::swap(out.add(j - 1), out.add(j));
            j -= 1;
        }
        i += 1;
    }
    n as i32
}

/// `find_name(prefix, name, out)`: the first file (in table order) whose path starts with `prefix` (NUL-terminated) and whose last path part is `name`,
/// its path NUL padded into `out`; its length, `E_NOT_FOUND` after the whole table is read and holds none, or the error of the read (`E_BUSY` while
/// an upload holds the store). It reads every entry (no cap on the files), unlike `list`.
pub unsafe fn find_name(prefix: *const u8, name: &[u8], out: *mut [u8; 64]) -> i32 {
    if !super::lock::acquire() {
        return E_BUSY;
    }
    let r = find_name_unlocked(prefix, name, out);
    super::lock::unlock();
    r
}

unsafe fn find_name_unlocked(prefix: *const u8, name: &[u8], out: *mut [u8; 64]) -> i32 {
    let (io, lay) = (reader(), layout());
    let fp = addr_of_mut!(DKRT_STORE_FP) as *mut u8;
    let mut plen = 0;
    while *prefix.add(plen) != 0 {
        plen += 1;
    }
    let r = store::stable(&io, &lay, fp, || {
        let t = open()?;
        let mut found = 0i32;
        let r = store::scan(&io, &lay, &t.h, stage(), STAGE_BYTES, |_, e| {
            if !listed(e, prefix, plen, Filter::Modules) {
                return true;
            }
            let len = store::path_len(e);
            let mut path = [0u8; 64];
            let mut k = 0;
            while k < len {
                path[k] = *e.add(k);
                k += 1;
            }
            if !dkrt_abi::store_path::last_part_is(&path, name) {
                return true;
            }
            *out = path;
            found = len as i32;
            false
        });
        if r != OK {
            return Err(r);
        }
        Ok(found)
    });
    match r {
        Ok(0) => E_NOT_FOUND,
        Ok(n) => n,
        Err(e) => e,
    }
}

/// `dkrt_store_read_sector(rel, out)`: one sector of the store (relative to its start) into `out`, 512 bytes, 16-byte aligned. 0 or a negative code.
pub unsafe fn read_sector(rel: u32, out: *mut u8) -> i32 {
    if crate::card::state() == CARD_TOO_SMALL {
        return E_NOT_FOUND;
    }
    if !super::lock::acquire() {
        return E_BUSY;
    }
    let r = store::read_sectors(&reader(), &layout(), rel, 1, out, 512);
    super::lock::unlock();
    r
}

/// `dkrt_store_stat(path, out)`: where a store file is, for an adapter that streams it. `E_NOT_FOUND` for no store, no such file and a folder.
/// Leaves garbage in the staging buffer (the table is scanned through it), so only call it at the start of a load.
#[no_mangle]
pub unsafe extern "C" fn dkrt_store_stat(path: *const u8, out: *mut DkrtStoreFile) -> i32 {
    if path.is_null() || out.is_null() {
        return E_CONTRACT;
    }
    crate::state::init_once();
    match find(path) {
        Ok(f) => {
            let o = &mut *out;
            o.first = f.first;
            o.size = f.size;
            o.sha = f.sha;
            OK
        }
        Err(e) => e,
    }
}

/// `dkrt_store_read(f, sector, count, dst)`: `count` sectors of the file `f` from its sector `sector` into `dst` (16-byte aligned, 512 bytes a
/// sector). No sha check and no staging buffer. Run in the task that owns the card reads (the UI loop).
#[no_mangle]
pub unsafe extern "C" fn dkrt_store_read(f: *const DkrtStoreFile, sector: u32, count: u32, dst: *mut u8) -> i32 {
    if f.is_null() || dst.is_null() || (dst as usize) & 15 != 0 {
        return E_CONTRACT;
    }
    crate::state::init_once();
    if crate::card::state() == CARD_TOO_SMALL {
        return E_NOT_FOUND;
    }
    let f = &*f;
    let rel = match store::file_span(f.first, f.size, sector, count) {
        Ok(r) => r,
        Err(e) => return e,
    };
    if !super::lock::acquire() {
        return E_BUSY;
    }
    let r = store::read_sectors(&reader(), &layout(), rel, count, dst, count as usize * 512);
    super::lock::unlock();
    r
}

/// `lease(owner, path, out)`: look the store file `path` up and lease its extent, in one store-lock hold, so that the extent in `out` is the
/// committed one and stays unchanged until the lease ends: the writer refuses to replace or delete the file and never gives its sectors to another
/// file. The id (> 0), `E_NOT_FOUND` (no store, no such file, a folder), `E_NOMEM` (the table is full: fail the load, do not wait) or `E_BUSY`.
/// The table is scanned through the SDK buffers of the store writer, which the store lock guards: the staging buffer is not touched, so any task
/// may lease (the sample pool's window step does, in the worker task).
pub unsafe fn lease(owner: u8, path: *const u8, out: &mut DkrtStoreFile) -> i32 {
    if !super::lock::acquire() {
        return E_BUSY;
    }
    let b = super::lock::sdk_bufs();
    let found = match open_in(b.scr, b.tin, super::lock::TBL_CAP) {
        Ok(t) => match store::find(&reader(), &layout(), &t.h, b.tin, super::lock::TBL_CAP, path) {
            Ok(Some((_, e))) if e.kind == KIND_FILE => Ok(e),
            Ok(_) => Err(E_NOT_FOUND),
            Err(e) => Err(e),
        },
        Err(e) => Err(e),
    };
    let r = match found {
        Ok(e) => {
            let id = super::lock::lease_take(owner, e.first, e.size);
            if id > 0 {
                out.first = e.first;
                out.size = e.size;
                out.sha = e.sha;
            }
            id
        }
        Err(e) => e,
    };
    super::lock::unlock();
    r
}

/// `dkrt_store_lease(path, out)`: `dkrt_store_stat` that also leases the file's extent for the caller (an adapter, owner 0) until
/// `dkrt_store_unlease(id)`. The id (> 0) or a negative code (see `lease`).
#[no_mangle]
pub unsafe extern "C" fn dkrt_store_lease(path: *const u8, out: *mut DkrtStoreFile) -> i32 {
    if path.is_null() || out.is_null() {
        return E_CONTRACT;
    }
    crate::state::init_once();
    lease(0, path, &mut *out)
}

/// `dkrt_store_unlease(id)`: end a lease (any task, no waiting). An id that is not live is ignored.
#[no_mangle]
pub unsafe extern "C" fn dkrt_store_unlease(id: i32) {
    super::lock::lease_end(id);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file under 64 bytes has no head, and no sector is read for it (the host has no card: a read would not be answered).
    #[test]
    fn a_file_under_64_bytes_has_no_head() {
        for size in [0, 1, 10, 63] {
            let f = File { first: 0, size, sha: [0; 32] };
            let mut head = [0xaau8; 64];
            assert_eq!(unsafe { read_head(&f, &mut head) }, Err(E_READ), "size {size}");
            assert_eq!(head, [0xaa; 64], "size {size}: nothing is written");
        }
    }
}
