//! DT2 sample reads through the firmware file system (`Backend::Fs`): the device tables `dkrt_sample_fs` and `dkrt_files_fs` of the runtime's assembly
//! objects (DT2 1.17 anchors; zeros on a device without them). This file names no firmware crate and no device id; `route.rs` decides when it is used.
//! Every call runs in task context.

use core::ptr::{addr_of, addr_of_mut, read_volatile};
use dkrt_abi::storage::{DirEntry, EntryKind, ListStep, Stat, HASH_ELEKTRON32, HASH_NONE};
use dkrt_sdk::{E_BAD_STORE, E_NOT_FOUND, E_READ, E_SEL_PATH, E_TOO_BIG};

extern "C" {
    // Per-device tables of the runtime's assembly objects (DT2 1.17 anchors; zeros on DN2). Only this file declares them.
    static dkrt_sample_fs: [u32; 4];
    static dkrt_files_fs: [u32; 3];
}

type FsOpen = unsafe extern "C" fn(*const u8, *const u8, *mut u32) -> i32;
type FsRead = unsafe extern "C" fn(*mut u8, u32, *mut u32) -> i32;
type FsClose = unsafe extern "C" fn(*mut u32) -> i32;
type FsSize = unsafe extern "C" fn(u32) -> u32;
type FsMeta = unsafe extern "C" fn(u32, *mut u32) -> i32;
type DirOpen = unsafe extern "C" fn(*const u8, *mut u32) -> i32;
type DirNext = unsafe extern "C" fn(*mut u32, *mut u8) -> i32;


fn dir_stat() -> Stat {
    Stat { kind: EntryKind::Dir, size: 0, hash_kind: HASH_NONE, hash_valid: false, hash: [0; 32] }
}

// ---- DT2 firmware file system ------------------------------------------------------------------------------------------------

/// The path of the firmware file system: `/` and the root-relative `path`, NUL-terminated. `out` holds at least `path.len() + 2` bytes.
pub(crate) fn fs_key(path: &[u8], out: &mut [u8]) -> Result<(), i32> {
    if path.is_empty() || path.contains(&0) {
        return Err(E_SEL_PATH);
    }
    if path.len() > 255 {
        return Err(E_TOO_BIG);
    }
    out[0] = b'/';
    out[1..1 + path.len()].copy_from_slice(path);
    out[1 + path.len()] = 0;
    Ok(())
}

/// The file hash of the Elektron file table as the firmware's metadata call reports it (words: id, odd hash, physical size, node): the odd hash is
/// the first 4 bytes, big-endian, the rest is 0.
fn fs_hash(values: &[u32; 4]) -> [u8; 32] {
    let mut h = [0u8; 32];
    h[..4].copy_from_slice(&values[1].to_be_bytes());
    h
}

#[derive(Clone, Copy)]
struct Fs {
    open: FsOpen,
    read: FsRead,
    close: FsClose,
    size: FsSize,
    meta: Option<FsMeta>,
}

// Host tests: the firmware file system functions of this thread (the device table holds 32-bit code addresses, which a host function pointer does not fit).
#[cfg(test)]
thread_local! {
    static TEST_FS: core::cell::Cell<Option<Fs>> = const { core::cell::Cell::new(None) };
}

/// Host tests: serve `ROOT_SAMPLES` of a device with the firmware file system from these functions instead of the device table.
#[cfg(test)]
pub(crate) fn test_install_fs(open: FsOpen, read: FsRead, close: FsClose, size: FsSize, meta: Option<FsMeta>) {
    TEST_FS.with(|c| c.set(Some(Fs { open, read, close, size, meta })));
}

/// Host tests: back to the device table (zeros in the host fixture: `E_NOT_FOUND`).
#[cfg(test)]
pub(crate) fn test_remove_fs() {
    TEST_FS.with(|c| c.set(None));
}

unsafe fn fs_table() -> Result<Fs, i32> {
    #[cfg(test)]
    if let Some(fs) = TEST_FS.with(|c| c.get()) {
        return Ok(fs);
    }
    let table = addr_of!(dkrt_sample_fs) as *const u32;
    let (open_addr, read_addr, close_addr, size_addr) =
        (read_volatile(table), read_volatile(table.add(1)), read_volatile(table.add(2)), read_volatile(table.add(3)));
    let meta_addr = read_volatile((addr_of!(dkrt_files_fs) as *const u32).add(2));
    if open_addr == 0 || read_addr == 0 || close_addr == 0 || size_addr == 0 {
        return Err(E_NOT_FOUND);
    }
    Ok(Fs {
        open: core::mem::transmute(open_addr as usize),
        read: core::mem::transmute(read_addr as usize),
        close: core::mem::transmute(close_addr as usize),
        size: core::mem::transmute(size_addr as usize),
        meta: if meta_addr != 0 { Some(core::mem::transmute(meta_addr as usize)) } else { None },
    })
}

/// Open the file `z` (NUL-terminated), run `f` on the handle, close it. `E_NOT_FOUND` when it does not open; a failed close is `E_READ` (as before).
unsafe fn fs_with<T>(fs: &Fs, z: &[u8], f: impl FnOnce(&Fs, &mut [u32; 4]) -> Result<T, i32>) -> Result<T, i32> {
    let mut handle = [0u32; 4];
    if (fs.open)(z.as_ptr(), b"r\0".as_ptr(), handle.as_mut_ptr()) != 0 {
        return Err(E_NOT_FOUND);
    }
    let result = f(fs, &mut handle);
    if (fs.close)(handle.as_mut_ptr()) != 0 {
        return Err(E_READ);
    }
    result
}

/// The size of an open file (`u32::MAX` is the firmware's failure).
unsafe fn fs_size(fs: &Fs, handle: &mut [u32; 4]) -> Result<u32, i32> {
    let size = (fs.size)(handle[0]);
    if size == u32::MAX {
        return Err(E_READ);
    }
    Ok(size)
}

/// Read exactly `n` bytes at the file position into `dst`.
unsafe fn fs_read_exact(fs: &Fs, handle: &mut [u32; 4], dst: *mut u8, n: usize) -> Result<(), i32> {
    let mut done = 0usize;
    while done < n {
        let want = core::cmp::min(n - done, 0x10000);
        let got = (fs.read)(dst.add(done), want as u32, handle.as_mut_ptr());
        if got <= 0 || got as usize > want {
            return Err(E_READ);
        }
        done += got as usize;
    }
    Ok(())
}


/// The firmware file system's half of `storage_adapter::stat`.
pub(crate) unsafe fn stat(path: &[u8]) -> Result<Stat, i32> {
    let mut z = [0u8; 258];
    fs_key(path, &mut z)?;
    let fs = fs_table()?;
    fs_with(&fs, &z, |fs, handle| {
        let size = fs_size(fs, handle)?;
        let mut st = Stat { kind: EntryKind::File, size, hash_kind: HASH_NONE, hash_valid: false, hash: [0; 32] };
        if let Some(meta) = fs.meta {
            let mut values = [0u32; 4];
            if meta(handle[0], values.as_mut_ptr()) == 0 && values[2] == size {
                st.hash_kind = HASH_ELEKTRON32;
                st.hash_valid = true;
                st.hash = fs_hash(&values);
            }
        }
        Ok(st)
    })
}

/// The firmware file system's half of `storage_adapter::stat_head`.
pub(crate) unsafe fn stat_head(path: &[u8], out: &mut [u8; 64]) -> Result<(Stat, usize), i32> {
    let mut z = [0u8; 258];
    fs_key(path, &mut z)?;
    let fs = fs_table()?;
    fs_with(&fs, &z, |fs, handle| {
        let size = fs_size(fs, handle)?;
        let mut st = Stat { kind: EntryKind::File, size, hash_kind: HASH_NONE, hash_valid: false, hash: [0; 32] };
        if let Some(meta) = fs.meta {
            let mut values = [0u32; 4];
            if meta(handle[0], values.as_mut_ptr()) == 0 && values[2] == size {
                st.hash_kind = HASH_ELEKTRON32;
                st.hash_valid = true;
                st.hash = fs_hash(&values);
            }
        }
        if size < 64 {
            return Err(E_READ);
        }
        fs_read_exact(fs, handle, out.as_mut_ptr(), 64).map_err(|_| E_READ)?;
        Ok((st, 64))
    })
}

/// The firmware file system's half of `storage_adapter::read_all`.
pub(crate) unsafe fn read_all(path: &[u8], dst: *mut u8, cap: u32) -> Result<u32, i32> {
    let mut z = [0u8; 258];
    fs_key(path, &mut z)?;
    let fs = fs_table()?;
    fs_with(&fs, &z, |fs, handle| {
        let size = fs_size(fs, handle)?;
        if size > cap {
            return Err(E_TOO_BIG);
        }
        fs_read_exact(fs, handle, dst, size as usize)?;
        Ok(size)
    })
}

// ---- listing -----------------------------------------------------------------------------------------------------------------

pub(crate) const TAG_FS: u32 = 0x4653_0001;

/// DT2 listing state: the firmware's directory state (4 words), the output buffer of its `next` (0x114 bytes: id, kind, ..., name at +20) and the path
/// handed to its directory open (the firmware may keep the pointer in its directory state, so the path lives here as long as the listing).
#[repr(C)]
pub(crate) struct Dt2List {
    pub(crate) tag: u32,
    /// The firmware's `next` has given the end: later calls give `End` without calling it.
    pub(crate) ended: bool,
    pub(crate) dir_state: [u32; 4],
    pub(crate) out: [u8; 0x114],
    pub(crate) path: [u8; 258],
}


/// Start listing `dir` (root-relative, checked by the caller for length) into `state`.
pub(crate) unsafe fn list_open(dir: &[u8], state: *mut u8) -> Result<(), i32> {
    if dir.contains(&0) {
        return Err(E_SEL_PATH);
    }
    if dir.len() > 255 {
        return Err(E_TOO_BIG);
    }
    let table = addr_of!(dkrt_files_fs) as *const u32;
    let (open, next, meta) = (read_volatile(table), read_volatile(table.add(1)), read_volatile(table.add(2)));
    if open == 0 || next == 0 || meta == 0 {
        return Err(E_NOT_FOUND);
    }
    let l = state as *mut Dt2List;
    addr_of_mut!((*l).tag).write(0);
    addr_of_mut!((*l).ended).write(false);
    addr_of_mut!((*l).dir_state).write([0; 4]);
    addr_of_mut!((*l).out).write([0; 0x114]);
    addr_of_mut!((*l).path).write([0; 258]);
    let path = &mut (*l).path;
    path[0] = b'/';
    path[1..1 + dir.len()].copy_from_slice(dir);
    let open: DirOpen = core::mem::transmute(open as usize);
    if open(path.as_ptr(), (*l).dir_state.as_mut_ptr()) != 0 {
        return Err(E_NOT_FOUND);
    }
    (*l).tag = TAG_FS;
    Ok(())
}

/// One firmware `next` call: `.` and `..` give `Pending`, a file's size and hash come from the metadata call by entry id.
pub(crate) unsafe fn list_next(state: *mut u8, out: &mut DirEntry) -> Result<ListStep, i32> {
    let l = &mut *(state as *mut Dt2List);
    if l.ended {
        return Ok(ListStep::End);
    }
    let table = addr_of!(dkrt_files_fs) as *const u32;
    let (next, meta) = (read_volatile(table.add(1)), read_volatile(table.add(2)));
    if next == 0 || meta == 0 {
        return Err(E_NOT_FOUND);
    }
    let next: DirNext = core::mem::transmute(next as usize);
    let meta: FsMeta = core::mem::transmute(meta as usize);
    let buf = &mut l.out;
    {
        buf.fill(0);
        let rc = next(l.dir_state.as_mut_ptr(), buf.as_mut_ptr());
        if rc == 0 {
            l.ended = true;
            return Ok(ListStep::End);
        }
        if rc < 0 {
            return Err(E_READ);
        }
        if rc != 1 {
            return Err(E_BAD_STORE);
        }
        let name = &buf[20..];
        let n = name.iter().position(|b| *b == 0).ok_or(E_BAD_STORE)?;
        if &name[..n] == b"." || &name[..n] == b".." {
            return Ok(ListStep::Pending);
        }
        if n == 0 || name[..n].contains(&b'/') {
            return Err(E_BAD_STORE);
        }
        *out = DirEntry::empty();
        out.name[..n].copy_from_slice(&name[..n]);
        out.name_len = n as u16;
        let kind = u16::from_be_bytes([buf[4], buf[5]]);
        if kind == 1 {
            out.stat = dir_stat();
            return Ok(ListStep::Entry);
        }
        out.stat = Stat { kind: EntryKind::File, size: 0, hash_kind: HASH_NONE, hash_valid: false, hash: [0; 32] };
        let id = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let mut values = [0u32; 4];
        if meta(id, values.as_mut_ptr()) == 0 {
            out.stat.size = values[2];
            out.stat.hash_kind = HASH_ELEKTRON32;
            out.stat.hash_valid = true;
            out.stat.hash = fs_hash(&values);
        }
        Ok(ListStep::Entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fs_key_is_rooted_and_terminated() {
        let mut z = [0xffu8; 258];
        fs_key(b"KICKS/K1", &mut z).unwrap();
        assert_eq!(&z[..10], b"/KICKS/K1\0");
        assert_eq!(fs_key(b"", &mut z), Err(E_SEL_PATH));
        assert_eq!(fs_key(b"a\0", &mut z), Err(E_SEL_PATH));
        assert!(fs_key(&[b'a'; 255], &mut z).is_ok());
        assert_eq!(fs_key(&[b'a'; 256], &mut z), Err(E_TOO_BIG));
    }

    #[test]
    fn an_ended_dt2_listing_gives_end_without_the_firmware() {
        let mut l = Dt2List { tag: TAG_FS, ended: true, dir_state: [0; 4], out: [0; 0x114], path: [0; 258] };
        let mut e = DirEntry::empty();
        let r = unsafe { list_next(&mut l as *mut Dt2List as *mut u8, &mut e) };
        assert!(matches!(r, Ok(ListStep::End)));
    }

    #[test]
    fn fs_hash_is_the_odd_hash_first() {
        let h = fs_hash(&[1, 0x0102_0304, 9, 0]);
        assert_eq!(&h[..4], &[1, 2, 3, 4]);
        assert!(h[4..].iter().all(|b| *b == 0));
    }
}
