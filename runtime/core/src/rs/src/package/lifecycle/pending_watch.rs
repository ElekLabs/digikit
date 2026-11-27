//! What the log says about a take-down that waits for a busy restore: one note when the wait starts or changes (the task it waits for, or its result) and one when it ends. The retry
//! of every UI pass is quiet (`QUIET_OP`); this owner keeps the last state it noted.
use dkrt_abi::package::PackageId;
use dkrt_sdk::{E_BUSY, OK};

/// The note a changed wait adds to the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Note {
    pub(super) package: PackageId,
    /// `E_BUSY`: still waiting; `OK`: the take-down ended; another code: it ended with that failure.
    pub(super) status: i32,
    /// The task a busy restore waits for (its place in the port file's list), else 0.
    pub(super) task: u32,
}

impl Note {
    pub(super) fn waiting(&self) -> bool {
        self.status == E_BUSY
    }
}

/// The last state of the pending take-down that was noted.
pub(super) struct PendingWatch {
    seen: Option<Note>,
}

impl PendingWatch {
    pub(super) const fn new() -> Self {
        PendingWatch { seen: None }
    }

    /// One retry of the take-down of `package` ended with `status` (`task`: what a busy restore waits for). The note to log, `None` when nothing changed since the last one.
    pub(super) fn observe(&mut self, package: PackageId, status: i32, task: Option<u32>) -> Option<Note> {
        let now = Note { package, status, task: if status == E_BUSY { task.unwrap_or(0) } else { 0 } };
        if self.seen == Some(now) {
            return None;
        }
        self.seen = if status == E_BUSY { Some(now) } else { None };
        Some(now)
    }

    /// The take-down that is being waited for, as noted last; `None` when none waits.
    pub(super) fn waiting_for(&self) -> Option<PackageId> {
        self.seen.map(|n| n.package)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hundred_busy_passes_note_the_start_and_the_end_only() {
        let mut w = PendingWatch::new();
        let p = PackageId(9);
        let mut notes = 0;
        assert_eq!(w.waiting_for(), None);
        for _ in 0..100 {
            notes += w.observe(p, E_BUSY, Some(1)).is_some() as u32;
            assert_eq!(w.waiting_for(), Some(p), "visible while it waits");
        }
        let done = w.observe(p, OK, None).unwrap();
        notes += 1;
        assert!(!done.waiting());
        assert_eq!(notes, 2);
        assert_eq!(w.waiting_for(), None);
    }

    #[test]
    fn a_change_of_task_or_result_is_noted_again() {
        let mut w = PendingWatch::new();
        let p = PackageId(9);
        assert!(w.observe(p, E_BUSY, Some(0)).is_some());
        assert!(w.observe(p, E_BUSY, Some(0)).is_none());
        assert_eq!(w.observe(p, E_BUSY, Some(1)).map(|n| n.task), Some(1));
        assert!(w.observe(p, -5, None).is_some_and(|n| !n.waiting() && n.status == -5));
        assert_eq!(w.waiting_for(), None);
    }
}
