//! The SDK front end of the store writer (`file_*` of api 1.10, `dk_files_commit` and `dk_files_pending`): path and root checks through `route.rs`, then the
//! store writer (`store_writer.rs`) or the +Drive backend (`backends/plusdrive.rs`) for the DT2 samples root. The `entry.rs` imports call only this file.
//! The calls run in an I/O step: the running task is the I/O task and `io::step_owner` names the module whose job it is.

use super::backends::plusdrive::*;
use super::lock::{acquire, sdk_bufs, unlock};
use super::route::root_route;
use super::store_writer::*;
use crate::platform::PLATFORM;
use dkrt_abi::platform::Platform;
use dkrt_sdk::proto::*;
use dkrt_sdk::store;
use dkrt_sdk::{E_BUSY, E_CONTEXT, E_CONTRACT, OK};

/// `DK_FILES_*` of `<dk/platform/files.h>`.
pub(crate) const ROOT_SAMPLES: u32 = 1;
pub(crate) const ROOT_STORE: u32 = 2;
/// `DK_FILE_REPLACE` of `<dk/platform/file.h>`.
pub const FILE_REPLACE: u32 = 1;
/// `DK_FILE_DEFER`: the file is verified at close and named in the table at the next commit of pending files.
pub const FILE_DEFER: u32 = 2;
/// A store path with its root prefix: at most `store::PATH_MAX` bytes and the NUL.
const FULL_MAX: usize = 168;

/// The SDK calls run in an I/O step only: the running task is the I/O task and a step is running. The owner is the module whose job it is.
unsafe fn sdk_owner() -> Result<u8, i32> {
    match crate::io::step_owner() {
        Some(o) => {
            note_owner_call(); // the owner's last file call: the idle time of the pending files counts from here
            Ok(o)
        }
        None => Err(E_CONTEXT),
    }
}

/// `root`/`path` as one path in `full` (the store's: `store::path_ok`; the +Drive's: the same rules without a root prefix). `pd` is set for the
/// DT2 samples root, which the +Drive adapter serves. `E_CONTRACT`: no such root; `E_PATH`: no NUL in 160 bytes, or the path breaks the rules, or
/// (+Drive) it starts in the adapter's staging folder. Returns the length.
unsafe fn sdk_path(root: u32, path: *const u8, full: &mut [u8; FULL_MAX], pd: &mut bool) -> Result<usize, i32> {
    let (prefix, via_pd) = root_route(&PLATFORM, root)?;
    *pd = via_pd;
    if path.is_null() {
        return Err(E_PATH);
    }
    let mut n = 0;
    while n < 160 && *path.add(n) != 0 {
        n += 1;
    }
    if n >= 160 {
        return Err(E_PATH);
    }
    zero(full.as_mut_ptr(), FULL_MAX);
    copy(full.as_mut_ptr(), prefix.as_ptr(), prefix.len());
    copy(full.as_mut_ptr().add(prefix.len()), path, n);
    match store::path_ok(full.as_ptr(), FULL_MAX) {
        Some(l) => {
            if *pd && pd_staging(full.as_ptr(), l) {
                return Err(E_PATH);
            }
            Ok(l)
        }
        None => Err(E_PATH),
    }
}

/// `file_create`: a write session for a new file; the handle (> 0) or a status. `hash4`: null, or the expected `hash | 1`, big-endian.
/// `flags`: `FILE_REPLACE` or 0.
pub unsafe fn sdk_create(root: u32, path: *const u8, size: u32, hash4: *const u8, flags: u32) -> i32 {
    let owner = match sdk_owner() {
        Ok(o) => o,
        Err(e) => return e,
    };
    if flags & !(FILE_REPLACE | FILE_DEFER) != 0 {
        return E_CONTRACT;
    }
    let mut full = [0u8; FULL_MAX];
    let mut pd = false;
    let pl = match sdk_path(root, path, &mut full, &mut pd) {
        Ok(l) => l,
        Err(e) => return e,
    };
    if pd {
        return pd_create(owner, full.as_ptr(), size, hash4, flags & FILE_REPLACE); // the +Drive names its files at close
    }
    let want = if hash4.is_null() {
        None
    } else {
        let mut w = [0u8; 32];
        copy(w.as_mut_ptr(), hash4, 4);
        Some(w)
    };
    if !acquire() {
        return busy(E_BUSY);
    }
    let mut free = 0;
    if flags & FILE_DEFER != 0 {
        let m = make_room(&sdk_bufs(), size);
        if m != OK {
            unlock();
            return busy(m);
        }
    }
    let r = begin(&sdk_bufs(), SDK, owner, full.as_ptr(), pl, size, want, flags & FILE_REPLACE == 0, &mut free);
    unlock();
    if r != OK {
        return busy(r);
    }
    up().defer = flags & FILE_DEFER != 0;
    handle()
}

/// `dk_files_commit`: name the calling module's pending files in one commit. The DT2 samples root: finish the adapter's table rebuild.
pub unsafe fn sdk_commit() -> i32 {
    let owner = match sdk_owner() {
        Ok(o) => o,
        Err(e) => return e,
    };
    if !acquire() {
        return busy(E_BUSY);
    }
    let r = commit_pending(&sdk_bufs(), Some(owner));
    unlock();
    if r != OK {
        return busy(r);
    }
    if !PLATFORM.samples_in_store() && pd_present() {
        if up().pd || !pd_enter() {
            return busy(E_BUSY); // a +Drive file is open, or the adapter is in a call
        }
        let r = pd_commit();
        pd_leave();
        return r;
    }
    OK
}

/// `dk_files_pending`: the calling module's pending files.
pub unsafe fn sdk_pending() -> i32 {
    match sdk_owner() {
        Ok(o) => pend_count(o),
        Err(e) => e,
    }
}

/// `file_write`: `len` bytes of `buf` (16-byte aligned) at `offset`, the next byte, in pieces of `WRITE_PIECE`. The store lock is taken once for the
/// whole call: `E_BUSY` only when it could not be taken, before anything was written. A status, 0 when all went in.
pub unsafe fn sdk_write(h: i32, offset: u32, buf: *const u8, len: u32) -> i32 {
    let owner = match sdk_owner() {
        Ok(o) => o,
        Err(e) => return e,
    };
    if buf.is_null() || (buf as usize) & 15 != 0 {
        return E_CONTRACT;
    }
    if up().pd {
        return pd_write(owner, h, offset, buf, len);
    }
    if !acquire() {
        return busy(E_BUSY);
    }
    let mut done = 0u32;
    let mut r = OK;
    while done < len && r == OK {
        let n = if len - done > WRITE_PIECE as u32 { WRITE_PIECE as u32 } else { len - done };
        r = write(&sdk_bufs(), SDK, owner, h, offset.wrapping_add(done), buf.add(done as usize) as *mut u8, n, false);
        done += n;
    }
    unlock();
    r
}

/// `file_close`: end and commit; `out` (4 bytes, may be null) gets the device's file hash.
pub unsafe fn sdk_close(h: i32, out: *mut u8) -> i32 {
    let owner = match sdk_owner() {
        Ok(o) => o,
        Err(e) => return e,
    };
    if up().pd {
        return pd_close(owner, h, out);
    }
    if !acquire() {
        return busy(E_BUSY);
    }
    let r = close(&sdk_bufs(), SDK, owner, h, out);
    unlock();
    r
}

pub unsafe fn sdk_abort(h: i32) -> i32 {
    let owner = match sdk_owner() {
        Ok(o) => o,
        Err(e) => return e,
    };
    if up().pd {
        return pd_abort(owner, h);
    }
    if !acquire() {
        return busy(E_BUSY);
    }
    let r = abort(SDK, owner, h);
    unlock();
    r
}

pub unsafe fn sdk_delete(root: u32, path: *const u8) -> i32 {
    if let Err(e) = sdk_owner() {
        return e;
    }
    let mut full = [0u8; FULL_MAX];
    let mut pd = false;
    if let Err(e) = sdk_path(root, path, &mut full, &mut pd) {
        return e;
    }
    if pd {
        return pd_delete(full.as_ptr());
    }
    if !acquire() {
        return busy(E_BUSY);
    }
    let r = delete(&sdk_bufs(), full.as_ptr(), false);
    unlock();
    busy(r)
}
