//! DT2 sample writes through the +Drive adapter (`Backend::PlusDrive`, `core/plusdrive-write`, `dkrt_pdw.h`): the staged file the adapter holds, the boot
//! recovery of its staging folder and the deferred table rebuild. The session record is the store writer's (`store_writer::up`); `route.rs` decides when this
//! file is used and `sdk_files.rs` calls it.

use super::super::lock::{acquire, unlock};
use super::super::store_writer::*;
use crate::platform::PLATFORM;
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use dkrt_abi::platform::{Platform, StagedWriter};
use dkrt_sdk::proto::*;
use dkrt_sdk::{E_BUSY, E_CONTRACT, OK};

extern "C" {
    // Optional symbols of the adapter (`dkrt_pdw.h`; `[object] optional` in module.toml): 0 when it is not in the image.
    fn dkpw_version() -> u32;
    fn dkpw_begin(path: *const u8, size: u32, hash4: *const u8, flags: u32) -> i32;
    fn dkpw_write(buf: *const u8, len: u32) -> i32;
    fn dkpw_close(hash_out: *mut u8) -> i32;
    fn dkpw_abort();
    fn dkpw_delete(path: *const u8) -> i32;
    fn dkpw_recover(out: *mut u32) -> i32;
    fn dkpw_commit() -> i32;
    fn dkpw_staging() -> *const u8;
}

/// The adapter may hold a staged file (`dkpw_begin` succeeded and no close or abort since).
static mut PD_ACTIVE: bool = false;
/// Test knob: when not 0, the adapter stops dead at that step of the next +Drive write (`dkrt_pdw.h`), as a power cut would; 0 at every boot.
#[no_mangle]
pub static mut DKRT_PD_CUT: u32 = 0;
/// The boot recovery of the staging folder ran.
static mut PD_RECOVERED: bool = false;
/// The staging folder may exist: the adapter keeps it for a whole upload session. `pd_tick` removes it (the adapter's recovery call removes an
/// empty folder) once no write session has run for `PD_STAGE_IDLE` blocks.
static mut PD_STAGE: bool = false;
/// Block clock at the last start or end of a +Drive write session.
static mut PD_STAGE_AT: u32 = 0;
/// 2 s (1500 blocks a second): longer than the gap between the files of one upload, short enough that the folder is gone soon after it.
const PD_STAGE_IDLE: u32 = 2 * 1500;

pub(crate) fn pd_present() -> bool {
    StagedWriter::present(&PLATFORM)
}

/// Take the adapter for one call while no write session is open (the UI pass). The session test and the claim of the adapter are one step under
/// the irq mask, so a `pd_create` that claims the session meanwhile finds the adapter busy and gives up.
fn pd_enter_idle() -> bool {
    let sr = crate::platform::irq_mask();
    let ok = unsafe { !read_volatile(addr_of!(PD_BUSY)) && up().kind == NONE };
    if ok {
        unsafe { write_volatile(addr_of_mut!(PD_BUSY), true) };
    }
    crate::platform::irq_restore(sr);
    ok
}

/// Take the adapter for one call. False: another call is running.
pub(crate) fn pd_enter() -> bool {
    let sr = crate::platform::irq_mask();
    let free = unsafe { !read_volatile(addr_of!(PD_BUSY)) };
    if free {
        unsafe { write_volatile(addr_of_mut!(PD_BUSY), true) };
    }
    crate::platform::irq_restore(sr);
    free
}

pub(crate) fn pd_leave() {
    unsafe { write_volatile(addr_of_mut!(PD_BUSY), false) };
}

fn pd_busy() -> bool {
    unsafe { read_volatile(addr_of!(PD_BUSY)) }
}

/// Is `full` (`l` bytes) the adapter's staging folder or inside it?
pub(crate) unsafe fn pd_staging(full: *const u8, l: usize) -> bool {
    let st = dkpw_staging();
    let mut n = 0;
    while *st.add(n) != 0 {
        n += 1;
    }
    // `staging` is the folder name with its leading '/': the path has none, so compare from the second byte
    let name = st.add(1);
    let m = n - 1;
    if l < m {
        return false;
    }
    let mut i = 0;
    while i < m {
        if *full.add(i) != *name.add(i) {
            return false;
        }
        i += 1;
    }
    l == m || *full.add(m) == b'/'
}

/// UI task, once per pass: remove the staged file of a dropped session, and run the staging recovery once after the filesystem is up.
pub(crate) unsafe fn tick() {
    if PLATFORM.samples_in_store() || !pd_present() {
        return;
    }
    if read_volatile(addr_of!(PD_DIRTY)) && pd_enter_idle() {
        dkpw_abort();
        write_volatile(addr_of_mut!(PD_DIRTY), false);
        write_volatile(addr_of_mut!(PD_ACTIVE), false);
        write_volatile(addr_of_mut!(PD_STAGE_AT), now());
        pd_leave();
    }
    // The upload session is over: remove the empty staging folder. `pd_enter_idle` holds the adapter and checks that no write session is open.
    let idle = read_volatile(addr_of!(PD_STAGE)) && now().wrapping_sub(read_volatile(addr_of!(PD_STAGE_AT))) >= PD_STAGE_IDLE;
    if (!read_volatile(addr_of!(PD_RECOVERED)) || idle) && pd_enter_idle() {
        let mut out = [0u32; 4];
        let r = dkpw_recover(out.as_mut_ptr());
        pd_leave();
        if r >= 0 {
            write_volatile(addr_of_mut!(PD_STAGE), false);
            write_volatile(addr_of_mut!(PD_RECOVERED), true);
            if out[0] != 0 || out[1] != 0 || out[2] != 0 || out[3] != 0 {
                // renamed into place, deleted, our leftovers that stayed, entries that are not ours (left alone)
                crate::dlog::ev(dkrt_sdk::logk::UPLOAD, 0, [crate::storage::upload_codes::PLUSDRIVE_REPLY, out[0], out[1], out[2], out[3]]);
            }
        }
    }
}

/// The adapter's deferred table rebuild (`dkpw_commit`); the caller holds the adapter (`pd_enter`).
pub(crate) unsafe fn pd_commit() -> i32 {
    dkpw_commit()
}

/// Claim the one write session for the +Drive. `E_BUSY`: a session is open or the store is in use.
pub(crate) unsafe fn pd_create(owner: u8, path: *const u8, size: u32, hash4: *const u8, flags: u32) -> i32 {
    if !pd_present() {
        return E_CONTRACT;
    }
    if !read_volatile(addr_of!(PD_RECOVERED)) {
        return busy(E_BUSY); // the boot recovery of the staging folder has not run yet
    }
    if !acquire() {
        return busy(E_BUSY);
    }
    let u = up();
    if u.kind != NONE {
        unlock();
        return busy(E_BUSY);
    }
    // the claim is made before the adapter call (which can take long): a SysEx BEGIN meanwhile finds an SDK session and says E_BUSY
    u.pd = true;
    u.size = size;
    u.next = 0;
    u.owner = owner;
    NEXT_HANDLE = NEXT_HANDLE.wrapping_add(1) & 0x7fff;
    u.handle = NEXT_HANDLE + 1;
    u.job = crate::io::step_job();
    u.last = now();
    u.kind = SDK;
    unlock();
    if !pd_enter() {
        u.kind = NONE;
        u.pd = false;
        return busy(E_BUSY);
    }
    if read_volatile(addr_of!(PD_ACTIVE)) {
        dkpw_abort(); // a staged file that no session owns
        write_volatile(addr_of_mut!(PD_ACTIVE), false);
    }
    let r = dkpw_begin(path, size, hash4, flags | (read_volatile(addr_of!(DKRT_PD_CUT)) << 8));
    if r == OK {
        write_volatile(addr_of_mut!(PD_ACTIVE), true);
    }
    // the adapter may have created the folder, also when the BEGIN failed
    write_volatile(addr_of_mut!(PD_STAGE), true);
    write_volatile(addr_of_mut!(PD_STAGE_AT), now());
    pd_leave();
    if r != OK {
        u.kind = NONE;
        u.pd = false;
        return busy(r);
    }
    u.last = now();
    u.handle
}

pub(crate) unsafe fn pd_write(owner: u8, h: i32, offset: u32, buf: *const u8, len: u32) -> i32 {
    if !mine(SDK, owner, h) {
        return E_STATE;
    }
    let u = up();
    if offset != u.next {
        return E_OFFSET;
    }
    let end = offset as u64 + len as u64;
    if end > u.size as u64 || (len & 511 != 0 && end != u.size as u64) {
        return E_LENGTH;
    }
    if len == 0 {
        return OK;
    }
    if !pd_enter() {
        return busy(E_BUSY);
    }
    let r = dkpw_write(buf, len);
    pd_leave();
    if r == OK {
        u.last = now();
        u.next = end as u32;
    }
    r
}

pub(crate) unsafe fn pd_close(owner: u8, h: i32, out: *mut u8) -> i32 {
    if !mine(SDK, owner, h) {
        return E_STATE;
    }
    if !pd_enter() {
        return busy(E_BUSY);
    }
    let u = up();
    let r = if u.next != u.size {
        dkpw_abort();
        E_LENGTH
    } else {
        dkpw_close(out)
    };
    write_volatile(addr_of_mut!(PD_ACTIVE), false);
    write_volatile(addr_of_mut!(PD_STAGE_AT), now());
    pd_leave();
    u.kind = NONE;
    u.pd = false;
    if r != OK {
        // a failed close can leave a journal and a staged file (a rename that failed after the destination was deleted): the UI pass runs the
        // recovery again before the next file is accepted
        write_volatile(addr_of_mut!(PD_RECOVERED), false);
    }
    r
}

pub(crate) unsafe fn pd_abort(owner: u8, h: i32) -> i32 {
    if !mine(SDK, owner, h) {
        return E_STATE;
    }
    if !pd_enter() {
        return busy(E_BUSY);
    }
    dkpw_abort();
    write_volatile(addr_of_mut!(PD_ACTIVE), false);
    write_volatile(addr_of_mut!(PD_STAGE_AT), now());
    pd_leave();
    let u = up();
    u.kind = NONE;
    u.pd = false;
    OK
}

pub(crate) unsafe fn pd_delete(path: *const u8) -> i32 {
    if !pd_present() {
        return E_CONTRACT;
    }
    if is_open() || !read_volatile(addr_of!(PD_RECOVERED)) || !pd_enter() {
        return busy(E_BUSY);
    }
    let r = dkpw_delete(path);
    pd_leave();
    r
}

