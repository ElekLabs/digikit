//! The store lock, the read leases and the shared buffers of a store operation: the lowest part of the storage layer that both the read side (`store.rs`)
//! and the write side (`store_writer.rs`) use, so that neither calls the other for them.
//!
//! Rules: no `match` on integers (a dense one becomes a jump table, which is not ColdFire code), no `core::fmt`, no slice indexing.

#[cfg(not(test))]
use crate::platform::PLATFORM;
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use dkrt_sdk::store::lease::Leases;
use dkrt_sdk::store::{self, Entry};
#[cfg(not(test))]
use dkrt_abi::platform::Tasking;
use dkrt_sdk::E_BUSY;

/// The table is streamed through two buffers of this size.
pub const TBL_CAP: usize = 8192;
/// Offsets in the SysEx upload buffer (`transport::DKRT_UPLOAD_BUF`): the two table buffers, inside the body region (a command that is not DATA
/// has a small body, copied out first), and the scratch region after the 32 KiB of body room (table read-back, header sector).
pub const IN_AT: usize = 1024;
pub const OUT_AT: usize = IN_AT + TBL_CAP;
pub const SCRATCH_AT: usize = 16 + MAX_CHUNK + 16;

/// Largest chunk of one write (the protocol's DATA chunk).
pub(crate) const MAX_CHUNK: usize = dkrt_sdk::proto::MAX_CHUNK as usize;

/// The three buffers of a store operation: the table's input and output streams (`TBL_CAP` bytes each) and a scratch (at least 512 bytes);
/// all 16-byte aligned and apart.
#[derive(Clone, Copy)]
pub struct Bufs {
    pub tin: *mut u8,
    pub tout: *mut u8,
    pub scr: *mut u8,
    /// The close's card read-back buffer (16-byte aligned, a multiple of 512 bytes): it overlaps the table buffers, which are free until the commit.
    pub rb: *mut u8,
    pub rb_cap: usize,
}

impl Bufs {
    /// The regions of the SysEx upload buffer `buf`.
    pub unsafe fn sysex(buf: *mut u8) -> Bufs {
        Bufs { tin: buf.add(IN_AT), tout: buf.add(OUT_AT), scr: buf.add(SCRATCH_AT), rb: buf.add(16), rb_cap: MAX_CHUNK }
    }
}

/// The SDK path's own buffers (the MIDI task uses the upload buffer at the same time). Static, so that the write-watch of the proofs leaves out the
/// card reads that land here by DMA (tests/regression/runtime/write-safety.ts).
pub(crate) const SDK_SCRATCH: usize = 1024;
#[repr(C, align(16))]
pub struct SdkBuf {
    tin: [u8; TBL_CAP],
    tout: [u8; TBL_CAP],
    scr: [u8; SDK_SCRATCH],
}
#[no_mangle]
pub static mut DKRT_SDK_BUF: SdkBuf = SdkBuf { tin: [0; TBL_CAP], tout: [0; TBL_CAP], scr: [0; SDK_SCRATCH] };

pub(crate) unsafe fn sdk_bufs() -> Bufs {
    let b = addr_of_mut!(DKRT_SDK_BUF);
    let tin = addr_of_mut!((*b).tin) as *mut u8;
    // `tin` and `tout` are adjacent (repr(C), 16-byte aligned): one 16 KiB read-back buffer
    Bufs { tin, tout: addr_of_mut!((*b).tout) as *mut u8, scr: addr_of_mut!((*b).scr) as *mut u8, rb: tin, rb_cap: 2 * TBL_CAP }
}

// ---- the lock -----------------------------------------------------------------------------------------------------------------

static mut STORE_LOCK: bool = false;

/// Take the store lock without blocking. False: another task is inside a store operation.
pub fn try_lock() -> bool {
    let sr = crate::platform::irq_mask();
    let free = unsafe { !read_volatile(addr_of!(STORE_LOCK)) };
    if free {
        unsafe { write_volatile(addr_of_mut!(STORE_LOCK), true) };
    }
    crate::platform::irq_restore(sr);
    free
}

/// How long a reader in the UI or boot task waits for the lock before it gives up (1 ms steps).
const WAIT_STEPS: u32 = 3000;

#[cfg(not(test))]
unsafe fn pause() {
    PLATFORM.sleep_us(1000);
}
#[cfg(test)]
unsafe fn pause() {}

/// Take the store lock for a reader in a task that may sleep (the UI or boot task, the I/O task); in the MIDI task a single try: `try_lock`, and while it is held (an SDK write or commit on the I/O task, a SysEx command
/// of the MIDI task) sleep 1 ms with the firmware's `sleep_us`, up to `WAIT_STEPS` times. False after that: the caller fails with `E_BUSY`
/// (the failure is logged); a reader never reads the table without the lock. Call only where `sleep_us` may block (a task, not an interrupt),
/// and do not wait on anything else while holding the lock.
pub unsafe fn acquire() -> bool {
    let mut n = 0;
    // the MIDI task must never sleep (it is the firmware's MIDI input and the host's replies): one try, then the caller says E_BUSY
    let waits = !on_midi_task();
    loop {
        if try_lock() {
            return true;
        }
        if !waits || n >= WAIT_STEPS {
            crate::dlog::ev(dkrt_sdk::logk::UPLOAD, dkrt_sdk::logk::F_ERROR, [0xb5, E_BUSY as u32, 0, 0, 0]);
            return false;
        }
        pause();
        n += 1;
    }
}

/// The MIDI task's TCB, noted when `dkrt_midi_rx` first runs (0 before). `acquire` does not sleep in that task, whatever code (a service command
/// handler, a store read called by one) is running in it.
static mut MIDI_TASK: u32 = 0;

/// `dkrt_midi_rx` calls it on entry.
pub unsafe fn note_midi_task() {
    if read_volatile(addr_of!(MIDI_TASK)) == 0 {
        write_volatile(addr_of_mut!(MIDI_TASK), crate::io::current_task());
    }
}

unsafe fn on_midi_task() -> bool {
    let m = read_volatile(addr_of!(MIDI_TASK));
    m != 0 && crate::io::current_task() == m
}

/// Is the store lock held now (an SDK call or a SysEx command is inside the store)?
pub fn is_locked() -> bool {
    unsafe { read_volatile(addr_of!(STORE_LOCK)) }
}

pub fn unlock() {
    let sr = crate::platform::irq_mask();
    unsafe { write_volatile(addr_of_mut!(STORE_LOCK), false) };
    crate::platform::irq_restore(sr);
}

// ---- read leases --------------------------------------------------------------------------------------------------------------

/// The leases (`dkrt_sdk::store::lease`): extents of store files that a reader still reads. A lease is taken under the store lock (`lease_take`), so
/// the extent is the committed one and no commit runs at the same time. The table changes with the interrupts masked: a release comes from the
/// reader's own task and never waits for the lock. Reads of the table (the refusals, the allocator's extents) hold the store lock.
static mut LEASES: Leases = Leases::new();

pub(crate) fn leases() -> &'static mut Leases {
    unsafe { &mut *addr_of_mut!(LEASES) }
}

/// Lease the data extent (`first`, `size` bytes) of a store file for `owner` (0: an adapter). The caller holds the store lock. The id (> 0), or
/// `E_NOMEM` when the table (`lease::MAX`) is full: the caller fails its load; it must not wait with the lock held.
pub fn lease_take(owner: u8, first: u32, size: u32) -> i32 {
    let sr = crate::platform::irq_mask();
    let id = leases().take(owner, first, store::sectors_of(size));
    crate::platform::irq_restore(sr);
    id
}

/// Release lease `id` (any task, no lock): 0, or `E_NOT_FOUND`.
pub fn lease_end(id: i32) -> i32 {
    let sr = crate::platform::irq_mask();
    let r = leases().end(id);
    crate::platform::irq_restore(sr);
    r
}

pub(crate) fn lease_drop_owner(owner: u8) {
    let sr = crate::platform::irq_mask();
    leases().drop_owner(owner);
    crate::platform::irq_restore(sr);
}

/// Is any lease live? (CLEAR refuses then.)
pub fn leased_any() -> bool {
    leases().any()
}

/// Does a lease hold the data of the file entry `e`? Replacing or deleting it is refused then (`E_FILE_IN_USE`).
pub(crate) fn leased(e: &Entry) -> bool {
    leases().guards(e.first, store::entry_sectors(e))
}

