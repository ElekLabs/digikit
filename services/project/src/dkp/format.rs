//! The layout of the file: magic, version, header offsets, section tags.
use dkrt_sdk::settings::Fmt;

pub const MAGIC: u32 = 0x444b_504a; // "DKPJ"
pub const VERSION: u16 = 1;
pub const HEADER: usize = 42;
/// The most bytes of a file and of a RAM image.
pub const AREA: usize = 16384;
pub const FMT: Fmt = Fmt {
    magic: MAGIC,
    version: VERSION,
    header: HEADER,
    area: AREA,
};

pub(super) const O_DEVICE: usize = 6;
pub(super) const O_SLOT: usize = 8;
pub(super) const O_NAME_LEN: usize = 9;
pub(super) const O_NAME: usize = 10;
pub(super) const O_STAMP: usize = 26;

pub const TAG_MODULES: u8 = 1;
pub const TAG_DATA: u8 = 2;
