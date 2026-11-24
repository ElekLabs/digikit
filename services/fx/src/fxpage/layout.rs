//! Sizes, record offsets and table versions of the effect pages.

pub(super) const INFO_MAGIC: u32 = 0x4658_5047; // "FXPG"
pub(super) const KNOBS: usize = 7;
pub(super) const WORDS: usize = 8; // the knobs and MIX
pub(super) const TEXT: usize = 24;
pub(super) const REP_SIZE: usize = 48;
pub(super) const KNOB_REC: usize = 52;
pub(super) const KNOB_AT: usize = 48;
pub(super) const RAW_MAX: i32 = 0x7f00;
pub(super) const RAW_F: f32 = 32512.0; // 0x7f00

/// The words of the image table that the service copies, and of its own table.
pub(super) const IMAGE_WORDS: usize = 64;
pub(super) const TAB_WORDS: usize = 72;
/// The version of the image table that has no firmware address in it (`w::VERSION`).
pub(super) const INFO_VERSION: u32 = 4;
pub(super) const ROW_BYTES: usize = 0x2c;
pub(super) const LIST_WORDS: usize = 8;
pub(super) const IDS_MAX: usize = 8;
pub(super) const DEFS_MAX: usize = 64;
