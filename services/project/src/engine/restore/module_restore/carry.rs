//! `ModuleRestore` over a reload: the list, the entry it is at and how far, so a restore that was under way goes on in the new copy.
use super::*;
use crate::engine::carry::{CarryIn, CarryOut};

impl ModuleRestore {
    /// The most bytes `export` writes.
    pub(in crate::engine) const EXPORT_MAX: usize =
        ModuleSet::EXPORT_MAX + 1 + 1 + 4 + 1 + MODULE_PATH_MAX + 1 + 4 + 4;

    pub(in crate::engine) fn export(&self, now: u32, out: &mut CarryOut) {
        self.pending.export(out);
        out.u8(self.at as u8);
        match self.step {
            Step::Next => out.u8(0),
            Step::Queue => out.u8(1),
            Step::Running => out.u8(2),
            Step::Waiting { .. } => out.u8(3),
        }
        out.deadline(
            now,
            match self.step {
                Step::Waiting { until } => until,
                _ => now,
            },
        );
        out.u8(self.file.as_bytes().len() as u8);
        out.bytes(self.file.as_bytes());
        out.u8(self.card_tries);
        out.u32(self.epoch);
        out.u32(self.slot);
    }

    pub(in crate::engine) fn import(&mut self, now: u32, from: &mut CarryIn) -> Option<()> {
        let pending = ModuleSet::import(from)?;
        let at = from.u8()? as usize;
        let tag = from.u8()?;
        let until = from.deadline(now)?;
        let len = from.u8()? as usize;
        let file = ModulePath::new(from.bytes(len)?)?;
        let card_tries = from.u8()?;
        let (epoch, slot) = (from.u32()?, from.u32()?);
        if at > pending.len() {
            return None;
        }
        self.step = match tag {
            0 => Step::Next,
            1 => Step::Queue,
            2 => Step::Running,
            3 => Step::Waiting { until },
            _ => return None,
        };
        self.pending = pending;
        self.at = at;
        self.file = file;
        self.card_tries = card_tries;
        self.epoch = epoch;
        self.slot = slot;
        Some(())
    }

    /// Nothing to restore, as at the start.
    pub(in crate::engine) fn start_empty(&mut self) {
        *self = ModuleRestore::new();
    }
}
