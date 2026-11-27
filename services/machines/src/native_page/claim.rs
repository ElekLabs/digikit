//! The claims on native pages of one load: each owner's pages are all of one kind, a page index is claimed once, and a v2 pair is complete.
use super::record::{is_kind, KIND, KIND_V2};

/// Bounded owner/index validation shared by registry checking and host tests.
pub struct PageSet<const N: usize> {
    owners: [u32; N],
    modes: [u32; N],
    masks: [u8; N],
    count: usize,
}
impl<const N: usize> PageSet<N> {
    pub const fn new() -> Self {
        Self {
            owners: [0; N],
            modes: [0; N],
            masks: [0; N],
            count: 0,
        }
    }
    pub fn claim(&mut self, kind: u32, owner: u32, page: u32) -> bool {
        if !is_kind(kind) || page > 1 || (kind == KIND && page != 1) {
            return false;
        }
        let slot = match self.owners[..self.count].iter().position(|&x| x == owner) {
            Some(slot) => slot,
            None => {
                if self.count == N {
                    return false;
                }
                let slot = self.count;
                self.count += 1;
                self.owners[slot] = owner;
                self.modes[slot] = kind;
                slot
            }
        };
        let bit = 1u8 << page;
        if self.modes[slot] != kind || self.masks[slot] & bit != 0 {
            return false;
        }
        self.masks[slot] |= bit;
        true
    }
    pub fn complete(&self) -> bool {
        (0..self.count).all(|i| self.modes[i] != KIND_V2 || self.masks[i] == 3)
    }
}

#[cfg(test)]
mod tests;
