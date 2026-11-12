//! Run-time switches of the speed levers (docs/plans/usb-fast.md). One RAM word, never persisted, all levers on at boot. The host sets bits through the
//! usb-fast service (STATUS body 'S' bit 0|2); a module through `dk_debug_speed_flags`, an adapter reads the word with `dkrt_speed_flags`. The bits are
//! fixed across the lanes that implement the levers.
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};

/// Bit 0, lever A: hash once. A store write with the expected hash (BEGIN's) does not hash the received bytes; the card read-back at close is the only hash.
pub const HASH_ONCE: u32 = 1;
/// Bit 2, lever C: the close's card read-back hashes on the helper task (`aux`) while the next piece is read.
pub const OVERLAP: u32 = 4;
/// The bits this word holds. Bit 1 (lever B, the USB receive ring) lives in the usb-bulk adapter and bit 3 (lever D, large card commands) in core/sdhc-large:
/// each adapter owns its switch, and the usb-fast STATUS 'S' mask composes the four.
pub const ALL: u32 = HASH_ONCE | OVERLAP;

static mut FLAGS: u32 = ALL;

/// The word.
pub fn flags() -> u32 {
    unsafe { read_volatile(addr_of!(FLAGS)) }
}

/// Is every bit of `bit` set?
pub fn on(bit: u32) -> bool {
    flags() & bit == bit
}

/// Clear the bits of `clear`, then set the bits of `set` (both limited to `ALL`); returns the new word.
pub fn update(clear: u32, set: u32) -> u32 {
    let sr = crate::irq_mask();
    let n = unsafe { (read_volatile(addr_of!(FLAGS)) & !(clear & ALL)) | (set & ALL) };
    unsafe { write_volatile(addr_of_mut!(FLAGS), n) };
    crate::irq_restore(sr);
    n
}

/// Adapter entry: the word.
#[no_mangle]
pub extern "C" fn dkrt_speed_flags() -> u32 {
    flags()
}
