//! How far each package of the manager has come: its [`State`] and how many services it is attached to.

use crate::state::State;
use crate::{PackageId, MAX_PACKAGES};

#[derive(Clone, Copy)]
struct Slot {
    package: PackageId,
    state: State,
    /// How many of the package's services (by ascending id) it is attached to.
    attached: usize,
}

/// The progress of every package that is not absent: placed, being brought up or down, active or waiting for a restore.
pub(super) struct Progress {
    slots: [Option<Slot>; MAX_PACKAGES],
}

impl Progress {
    pub(super) const fn new() -> Progress {
        Progress {
            slots: [None; MAX_PACKAGES],
        }
    }

    fn find(&self, p: PackageId) -> Option<usize> {
        self.slots
            .iter()
            .position(|s| matches!(s, Some(x) if x.package == p))
    }

    pub(super) fn state(&self, p: PackageId) -> Option<State> {
        self.find(p).and_then(|i| self.slots[i]).map(|s| s.state)
    }

    pub(super) fn attached(&self, p: PackageId) -> usize {
        self.find(p)
            .and_then(|i| self.slots[i])
            .map_or(0, |s| s.attached)
    }

    /// Sets the state of `p` (a new package has no services attached). A graph never holds more packages than slots.
    pub(super) fn set(&mut self, p: PackageId, state: State) {
        match self.find(p) {
            Some(i) => self.slots[i].as_mut().unwrap().state = state,
            None => {
                if let Some(free) = self.slots.iter().position(|s| s.is_none()) {
                    self.slots[free] = Some(Slot {
                        package: p,
                        state,
                        attached: 0,
                    });
                }
            }
        }
    }

    pub(super) fn set_attached(&mut self, p: PackageId, n: usize) {
        if let Some(i) = self.find(p) {
            self.slots[i].as_mut().unwrap().attached = n;
        }
    }

    pub(super) fn remove(&mut self, p: PackageId) {
        if let Some(i) = self.find(p) {
            self.slots[i] = None;
        }
    }

    /// The lowest-id package waiting for a restore.
    pub(super) fn pending(&self) -> Option<PackageId> {
        self.slots
            .iter()
            .flatten()
            .filter(|s| s.state == State::RestorePending)
            .map(|s| s.package)
            .min()
    }
}
