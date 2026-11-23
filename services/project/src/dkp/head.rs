//! The header of the file: the fields that name the project, and why a file is ignored.
use super::*;
use dkrt_sdk::settings::{get16, get32, put16, put32};

/// The fields of the header that name the project.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Head {
    pub device: u16,
    pub slot: u8,
    pub name_len: u8,
    pub name: [u8; 16],
    pub stamp: u32,
    pub counter: u32,
    pub used: u32,
}

/// Why a file is not used (the reason word of `PROJECT_IGNORED`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ignore {
    /// A bad crc32, magic, bounds or section.
    Format = 1,
    /// A file of another device.
    Device = 2,
    /// A version this service does not read.
    Version = 3,
    /// The file's save stamp is not the slot's.
    Stamp = 4,
    /// Not a file's reason: the store refused the listing of the slot's files, so the project could not be bound.
    Store = 5,
    /// Not a file's reason: a stock save into the slot whose load failed was skipped (it has no image to write, and its files stay as they are).
    SaveSkipped = 6,
    /// Not a file's reason: a stock save was dropped because the firmware's listing had no record of the slot within `REC_WAIT_MS`.
    SaveNoRecord = 7,
}

/// The header of `b` as it stands (no checks; `b` holds at least `HEADER` bytes).
pub fn head_of(b: &[u8]) -> Head {
    let mut name = [0u8; 16];
    let mut i = 0;
    while i < 16 {
        name[i] = b[O_NAME + i];
        i += 1;
    }
    Head {
        device: get16(b, O_DEVICE) as u16,
        slot: b[O_SLOT],
        name_len: b[O_NAME_LEN],
        name,
        stamp: get32(b, O_STAMP),
        counter: get32(b, FMT.o_seq()),
        used: get32(b, FMT.o_used()),
    }
}

/// Write the fields of `h` that name the project (not `used`, and not the crc32: `seal` does).
pub fn set_head(b: &mut [u8], h: &Head) {
    put16(b, O_DEVICE, h.device as u32);
    b[O_SLOT] = h.slot;
    b[O_NAME_LEN] = h.name_len;
    let mut i = 0;
    while i < 16 {
        b[O_NAME + i] = h.name[i];
        i += 1;
    }
    put32(b, O_STAMP, h.stamp);
}

/// Make `b` an empty image for this project: the header and no section.
pub fn reset(b: &mut [u8], device: u16, slot: u8, name: &[u8; 16], stamp: u32) {
    FMT.init(b, 0);
    let mut n = 16;
    while n > 0 && (name[n - 1] == 0) {
        n -= 1;
    }
    set_head(
        b,
        &Head {
            device,
            slot,
            name_len: n as u8,
            name: *name,
            stamp,
            counter: 0,
            used: HEADER as u32,
        },
    );
}

/// The write counter and the crc32 of a file's header: two files that agree on both hold the same bytes (the crc32 covers the counter). `b` holds a whole file.
pub fn identity(b: &[u8]) -> (u32, u32) {
    (head_of(b).counter, get32(b, FMT.o_crc()))
}

/// Set the write counter and the crc32 of an image whose bytes are final.
pub fn seal(b: &mut [u8], counter: u32) {
    FMT.seal(b, counter);
}
