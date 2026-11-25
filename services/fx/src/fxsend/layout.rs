//! Routes, limits and the word names of the info table of the send cells.

/// The routes of an effect (`dkrt_sdk::DkrtFx`), and the bits of the flags word of its record (docs/design/digikit-dkm.md).
pub(super) const ROUTE_MASTER: u8 = dkneeds::page::DK_ROUTE_MASTER as u8;
pub(super) const ROUTE_SEND: u8 = dkneeds::page::DK_ROUTE_SEND as u8;
pub(super) use dkneeds::{FXF_DEFAULT_SEND, FXF_NOMASTER, FXF_SEND};

/// Cells of the send page the image can have at most (`[limits.max] send_slots`).
pub const SEND_MAX: usize = dkrt_sdk::proto::CAPMAX_SEND_SLOTS as usize;
pub(super) const TRACKS: usize = 16;
/// UI-loop passes the loader counter must be still before the amounts are written back.
pub(super) const SETTLE: u32 = 4;
/// UI-loop passes a 0 must stay in a Sound array before it counts as a turn to 0: the sound loader zeroes the array at the start of a load and the loader counter moves at its
/// end (a pass in between sees the zero; the restore after the count overwrites it before it is taken).
pub(super) const ZERO_PASSES: u8 = 3;
/// Offset of the parameter array in a Sound object.
pub(super) const SOUND_ARRAY: u32 = 0x14;
pub(super) const KEY_MAX: usize = 48;

/// Words of the bound table of the effect pages (`fxpage::w`; byte offset / 4).
pub(super) mod w {
    pub const TPL_PERCENT: usize = 16;
    pub const CELLS: usize = 39;
    pub const ROW: usize = 40;
    pub const COUNT: usize = 41;
    pub const P_SEND: usize = 42;
    pub const IDX0: usize = 43;
    pub const TEMPLATE: usize = 45;
    pub const SOUND_OFF: usize = 46;
    pub const SOUND_STRIDE: usize = 47;
    pub const NSOUND: usize = 48;
}
