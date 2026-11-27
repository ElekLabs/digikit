use crate::steps::Steps;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Op {
    Unload,
    Reload,
}

/// A take-down that stopped at a busy restore and waits for [`PackageManager::retry_pending`](super::PackageManager::retry_pending): what is left to take
/// down, in order, and for a reload what to bring back. A failed bring-up whose unwind stopped at a busy restore leaves one too (the failed package, then
/// its orphans).
#[derive(Clone, Copy)]
pub(super) struct PendingTakeDown {
    pub(super) op: Op,
    pub(super) todo: Steps,
    pub(super) rebuild: Steps,
    pub(super) down: Steps,
}
