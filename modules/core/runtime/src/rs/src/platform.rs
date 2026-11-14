//! The platform under the runtime: the device constants, cache flush, interrupt mask, the firmware's block read and write (the one gated
//! card write), MIDI send, the firmware's counters and the big-endian and C-string helpers.

use crate::card;
use crate::state::{State, DKRT_STATE};
use core::ptr::{addr_of, addr_of_mut, read_volatile};
use dkrt_sdk::*;

extern "C" {
    /// Per-device constants, defined by `src/cf/<dev>/device.s` from the device file: the digikit id (`[digikit] id`) and the name the
    /// INFO reply gives (`[digikit] name`: 3 characters and a NUL, as a big-endian word).
    static dkrt_device_id: u32;
    static dkrt_device_name: u32;
    /// The image id patchkit stamps into the image (`src/cf/<dev>/device.s`; `packages/patchkit/src/imageid.rs`): 8 bytes, zero in an object.
    static dkrt_image_id: [u8; 8];
    /// `cpushl` over a byte range (both caches), `src/cf/tramp.s`.
    pub fn dkrt_flush(start: u32, len: u32);
    /// `src/cf/tramp.s`: mask interrupts (returns the old status register), restore it, and the soft reset.
    pub fn dkrt_irq_mask() -> u32;
    pub fn dkrt_irq_restore(sr: u32);
    pub fn dkrt_soft_reset() -> !;
}

pub fn st() -> &'static mut State {
    unsafe { &mut *addr_of_mut!(DKRT_STATE) }
}

pub fn flush(start: u32, len: u32) {
    unsafe { dkrt_flush(start, len) }
}

pub fn device() -> u32 {
    unsafe { read_volatile(addr_of!(dkrt_device_id)) }
}


/// The device's short name as the INFO reply gives it: 3 characters and a NUL.
pub fn device_name() -> u32 {
    unsafe { read_volatile(addr_of!(dkrt_device_name)) }
}

/// The 8-byte id of this image, as patchkit stamped it (zeros when the object was not patched in by it).
pub fn image_id() -> [u8; 8] {
    let p = addr_of!(dkrt_image_id) as *const u8;
    let mut o = [0u8; 8];
    let mut i = 0;
    while i < 8 {
        o[i] = unsafe { read_volatile(p.add(i)) };
        i += 1;
    }
    o
}

/// The word a firmware code literal points to, when the literal is a DDR address (a misplaced anchor reads nothing).
unsafe fn word_at_literal(lit: *const u32) -> Option<u32> {
    let l = core::hint::black_box(lit as usize) as u32;
    if l == 0 {
        return None;
    }
    let a = read_volatile(l as *const u32);
    if !(dkrt_sdk::device::DDR_CACHED_BASE..dkrt_sdk::device::DDR_CACHED_BASE + dkrt_sdk::device::DDR_BYTES).contains(&a) || a & 3 != 0 {
        return None;
    }
    Some(read_volatile(a as *const u32))
}

/// The firmware's DSP block counter (1500 per second, from boot; wraps), `None` when the anchor does not point at a DDR word.
/// `BLOCK_COUNTER_LIT` is the 4-byte address literal in the firmware's code, whose value is the address of the word.
#[cfg(not(test))]
pub unsafe fn fw_blocks() -> Option<u32> {
    word_at_literal(addr_of!(dkfw::link::BLOCK_ISR_COUNTER_LIT) as *const u32)
}

/// Bytes the firmware has handed to the USB stack for MIDI and not yet sent (0 when the host reads), `None` as for `fw_blocks`.
pub unsafe fn tx_in_flight() -> Option<u32> {
    word_at_literal(addr_of!(dkfw::midi::TX_INFLIGHT_LIT) as *const u32)
}

/// Is an OS upgrade stream being taken (finding 75)? The receive state object's byte at +0 is 0 when idle and 1 to 3 from the header on. False when the
/// anchor does not point at a DDR object, so the caller then acts as before.
pub unsafe fn os_upgrade_active() -> bool {
    matches!(word_at_literal(addr_of!(dkfw::midi::OS_UPGRADE_STATE_LIT) as *const u32), Some(w) if w >> 24 != 0)
}

pub unsafe fn block_read(sector: u32, bytes: u32, buf: *mut u8) -> i32 {
    dkfw::storage::block_read(sector, bytes, buf)
}

/// The firmware's block write, ungated. Only `block_write` and `card::claim` call it.
pub unsafe fn raw_block_write(sector: u32, bytes: u32, buf: *const u8) -> i32 {
    dkfw::storage::block_write(sector, bytes, buf)
}

/// The one function that writes the card. Write `bytes` (a multiple of 512) from `buf` (16-byte aligned) to `sector`: refused with `E_RANGE`
/// outside the store range of the device file, and with `E_CARD_RO` when the card's area is not ours (`card::admit`; finding 46). Every
/// runtime card write goes through here: the selection record and the boot counter (boot.rs), uploads (upload.rs), and the claim of a blank
/// range (card.rs, which calls `raw_block_write` for its own headers after the same checks).
pub unsafe fn block_write(sector: u32, bytes: u32, buf: *const u8) -> i32 {
    let g = card::admit(sector, bytes);
    if g != OK {
        return g;
    }
    card::info().writes = card::info().writes.wrapping_add(1);
    raw_block_write(sector, bytes, buf)
}

#[cfg(not(test))]
pub fn irq_mask() -> u32 {
    unsafe { dkrt_irq_mask() }
}

#[cfg(not(test))]
pub fn irq_restore(sr: u32) {
    unsafe { dkrt_irq_restore(sr) }
}

/// Send one complete MIDI message (F0 .. F7 for SysEx) to `port` (2 = USB). The firmware takes the MIDI TX mutex itself and spins
/// when the 4 KiB TX ring is full.
pub unsafe fn midi_send(len: u32, msg: *const u8, port: u32) {
    dkfw::midi::send(len, msg, port)
}

pub unsafe fn rd32(p: *const u8) -> u32 {
    ((*p as u32) << 24)
        | ((*p.add(1) as u32) << 16)
        | ((*p.add(2) as u32) << 8)
        | (*p.add(3) as u32)
}

pub unsafe fn wr32(p: *mut u8, v: u32) {
    *p = (v >> 24) as u8;
    *p.add(1) = (v >> 16) as u8;
    *p.add(2) = (v >> 8) as u8;
    *p.add(3) = v as u8;
}

/// Compare two NUL-terminated strings.
pub unsafe fn cstr_eq(a: *const u8, b: *const u8) -> bool {
    let mut i = 0;
    loop {
        let x = *a.add(i);
        if x != *b.add(i) {
            return false;
        }
        if x == 0 {
            return true;
        }
        i += 1;
    }
}

/// Length of a NUL-terminated string, at most `max`.
pub unsafe fn cstr_len(a: *const u8, max: usize) -> usize {
    let mut i = 0;
    while i < max && *a.add(i) != 0 {
        i += 1;
    }
    i
}
