//! Sizes, record offsets and table versions of the effect pages.

/// The magic of the bound table (word 0, set last).
pub(super) const INFO_MAGIC: u32 = 0x4658_5047; // "FXPG"
/// The value of the mark in the kit's array: the rows hold values (any other value: the array was not given its defaults).
pub(super) const FXP_MARK: u32 = 0x46a7;
/// The owner numbers of the blocks of `rows_info` this service fills (the device files' `[[rows.block]]`): the effect knobs (`WORDS` rows for each effect row) and the send knobs.
pub(super) const BLOCK_KNOBS: u32 = 2;
pub(super) const BLOCK_SEND: u32 = 3;
pub(super) const KNOBS: usize = 7;
pub(super) const WORDS: usize = 8; // the knobs and MIX
pub(super) const TEXT: usize = 24;
pub(super) const REP_SIZE: usize = 48;
pub(super) const KNOB_REC: usize = 52;
pub(super) const KNOB_AT: usize = 48;
pub(super) const RAW_MAX: i32 = 0x7f00;
pub(super) const RAW_F: f32 = 32512.0; // 0x7f00

/// The words of the table that the device facts and the blocks make, and of the whole table with the computed words.
pub(super) const IMAGE_WORDS: usize = 64;
pub(super) const TAB_WORDS: usize = 72;
pub(super) const ROW_BYTES: usize = 0x2c;
pub(super) const LIST_WORDS: usize = 8;
pub(super) const IDS_MAX: usize = 8;
pub(super) const DEFS_MAX: usize = 64;
