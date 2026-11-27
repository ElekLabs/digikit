//! Which slots have digikit data: one bit per slot. UI task only.

/// The set of slots whose `.dkp` file is in the store (bit `slot % 8` of byte `slot / 8`, the layout of the hooks' bitmaps). It is filled by one listing of `projects/` after the
/// start and kept by the work that changes files: a load, a save, a clear and a paste. Until the listing ended the set is not known.
pub struct DataIndex {
    bits: [u8; 16],
    known: bool,
}

impl DataIndex {
    pub const fn new() -> DataIndex {
        DataIndex {
            bits: [0; 16],
            known: false,
        }
    }

    /// Has the first listing ended?
    pub fn known(&self) -> bool {
        self.known
    }

    pub fn has(&self, slot: u32) -> bool {
        slot < 128 && self.bits[(slot / 8) as usize] & (1 << (slot % 8)) != 0
    }

    /// A piece of work found out whether `slot` has a file.
    pub fn set(&mut self, slot: u32, has: bool) {
        if slot >= 128 {
            return;
        }
        let (at, bit) = ((slot / 8) as usize, 1u8 << (slot % 8));
        if has {
            self.bits[at] |= bit;
        } else {
            self.bits[at] &= !bit;
        }
    }

    /// The listing ended with these slots: the set is known from now on.
    pub fn replace(&mut self, bits: [u8; 16]) {
        self.bits = bits;
        self.known = true;
    }
}

impl Default for DataIndex {
    fn default() -> Self {
        Self::new()
    }
}
