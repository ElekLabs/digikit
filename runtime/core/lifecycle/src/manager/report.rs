use super::refused::{Refused, RefusedList};
use crate::steps::Steps;
use crate::PackageId;

/// What an operation did, in the order it did it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub(super) brought_up: Steps,
    pub(super) taken_down: Steps,
    pub(super) refused: RefusedList,
    pub(super) pending: Option<PackageId>,
    pub(super) dsp_pending: Steps,
}

impl Report {
    /// The packages brought up.
    pub fn brought_up(&self) -> &Steps {
        &self.brought_up
    }

    /// The packages taken down: those of an unload or a reload, and the services a failed bring-up had started that nothing needed any more.
    pub fn taken_down(&self) -> &Steps {
        &self.taken_down
    }

    /// The packages that were not brought up and why: a cause comes before the packages that needed it.
    pub fn refused(&self) -> &[Refused] {
        self.refused.as_slice()
    }

    /// The packages whose DSP part is coming up (`ProviderHost::start_dsp` answered `true`), in bring-up order. A failure of one is reported with
    /// [`PackageManager::take_down_closure`](super::PackageManager::take_down_closure).
    pub fn dsp_pending(&self) -> &Steps {
        &self.dsp_pending
    }

    /// The package whose restore was busy when the operation ended: a take-down waits ([`PackageManager::retry_pending`](super::PackageManager::retry_pending))
    /// and every other operation is refused until it is finished. `None` when nothing waits.
    pub fn pending(&self) -> Option<PackageId> {
        self.pending
    }
}
