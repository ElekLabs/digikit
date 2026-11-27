//! `ModuleSet` over a reload.
use super::*;
use crate::engine::carry::{CarryIn, CarryOut};

impl ModuleSet {
    /// The most bytes `export` writes.
    pub(in crate::engine) const EXPORT_MAX: usize = 1 + MODULES_MAX * (1 + ID_MAX + 4);

    pub(in crate::engine) fn export(&self, out: &mut CarryOut) {
        out.u8(self.count as u8);
        let mut at = 0;
        while at < self.count {
            out.u8(self.ids[at].as_bytes().len() as u8);
            out.bytes(self.ids[at].as_bytes());
            out.u32(self.hashes[at]);
            at += 1;
        }
    }

    pub(in crate::engine) fn import(from: &mut CarryIn) -> Option<ModuleSet> {
        let count = from.u8()? as usize;
        if count > MODULES_MAX {
            return None;
        }
        let mut set = ModuleSet::new();
        let mut at = 0;
        while at < count {
            let len = from.u8()? as usize;
            let id = ModuleId::new(from.bytes(len)?)?;
            set.push(id, from.u32()?);
            at += 1;
        }
        Some(set)
    }
}
