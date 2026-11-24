//! The facts of the bound project that a planner reads.
use crate::engine::bound::*;
use crate::engine::SlotRecord;

/// What a planner reads of the bound project: a copy of the facts, so that a plan changes nothing.
#[derive(Clone, Copy)]
pub struct BoundView {
    pub slot: Slot,
    /// The file the image came from or was last written to.
    pub file: Option<SourceFile>,
    pub dirty: bool,
    /// Does the image hold data worth a file?
    pub has_data: bool,
    /// The bytes of the image's file.
    pub used: u32,
}

impl BoundView {
    /// The view after `Effect::AdoptForSave`, the one rule for it: a save into another slot takes the image with it, so the slot has no file yet and the image counts as changed.
    /// The planner reads it and `BoundProject::apply` installs it.
    pub fn adopted_for_save(self, slot: u32) -> BoundView {
        if self.slot.as_i32() == slot as i32 {
            return self;
        }
        BoundView {
            slot: Slot::new(slot),
            file: None,
            dirty: true,
            ..self
        }
    }

    /// Does the file the image came from carry the slot, stamp and name of the firmware's `record`?
    pub fn record_matches_file(&self, slot: u32, record: &SlotRecord) -> bool {
        if let Some(file) = self.file {
            return file.head.slot as u32 == slot
                && file.head.stamp == record.stamp
                && file.head.name == record.name;
        }
        false
    }
}
