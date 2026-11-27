//! How the bound project came to be bound, and how its start is named.
use crate::engine::bound::*;

/// What the service knows of the current project. The scope is on only for `Bound`; `LoadFailed` is off too (module writes go to the settings record), but remembers the slot whose
/// load the store refused: the firmware stays on that slot and calls the setter no more, so a stock save into it must not be taken for a save of an empty project.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)] // zero first: the service is a bss static
pub(super) enum Binding {
    /// No current project (the start, or the setter left the project).
    None,
    /// The project of this slot is current: the image belongs to it.
    Bound(u32),
    /// The load of `slot` ended with a store error; the image is empty and belongs to no project. The load is tried again quietly at `retry_at` (ms, wraps).
    LoadFailed { slot: u32, retry_at: u32 },
}

impl Binding {
    /// The slot of the bound project (`Slot::NONE` unless `Bound`).
    pub(super) fn bound_slot(self) -> Slot {
        if let Binding::Bound(slot) = self {
            return Slot::new(slot);
        }
        Slot::NONE
    }
    /// The slot whose load failed.
    pub(super) fn failed_slot(self) -> Option<u32> {
        if let Binding::LoadFailed { slot, .. } = self {
            return Some(slot);
        }
        None
    }
}

/// Is the project that was current at the start named yet? The stock setter is not called for a project that was already current, so the first save would look like a save-as
/// and archive its file: until a setter call or a read of the firmware names the project, the tick asks the firmware for its slot.
#[derive(Clone, Copy)]
#[repr(u8)] // zero first: the service is a bss static
pub(super) enum StartBind {
    Unnamed,
    Named,
}
