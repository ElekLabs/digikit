//! The slot of the current project, and the slot of the firmware's live mirror.

/// The slot of the firmware's live mirror: its stamp is the stamp of the project being edited.
pub const LIVE_MIRROR: u32 = 0x80;

/// The slot of the current project, or none. Stored as the slot plus one, so that none is zero and the whole state of the service is zero at the start (a static with a non-zero
/// value would put 35 KB of zeros in the file's data).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Slot(u32);

impl Slot {
    pub const NONE: Slot = Slot(0);
    pub const fn new(index: u32) -> Slot {
        Slot(index + 1)
    }
    pub fn is_none(self) -> bool {
        self.0 == 0
    }
    /// The slot index, `None` for none.
    pub fn index(self) -> Option<u32> {
        if self.0 == 0 {
            None
        } else {
            Some(self.0 - 1)
        }
    }
    /// The slot index, -1 for none.
    pub fn as_i32(self) -> i32 {
        self.0 as i32 - 1
    }
}
