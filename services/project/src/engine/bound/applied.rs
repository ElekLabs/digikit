//! What a plan asks of the bound project, and what came of it.
use crate::dkp::Ignore;
use crate::engine::bound::*;
use crate::engine::SlotRecord;

/// What a plan asks of the bound project; [`BoundProject::apply`] does it.
#[derive(Clone, Copy)]
pub enum Effect {
    /// Make `slot` current from `file`, a file of the slot that passed the checks (`record` is the firmware's record of the slot).
    BindFile {
        slot: u32,
        record: SlotRecord,
        file: SourceFile,
    },
    /// Make `slot` current with an empty image; `ignored` is why a file of the slot was refused (logged and warned once).
    BindEmpty {
        slot: u32,
        record: SlotRecord,
        ignored: Option<Ignore>,
    },
    /// The current project's slot was cleared: its image is empty and its file gone.
    EmptyCurrent { slot: u32, record: SlotRecord },
    /// A stock save into `slot` is about to write: when that is not the current slot the image goes with the save.
    AdoptForSave { slot: u32 },
}

/// What [`BoundProject::apply`] came to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Applied {
    /// Done: the plan's tasks may run.
    Done,
    /// The file of a load vanished or changed between the listing and the read: the slot was bound empty and the plan ends.
    Vanished,
    /// The store refused the read of a load's file with this status (busy or not): nothing changed, the caller handles it like a refused listing.
    Refused(i32),
}
