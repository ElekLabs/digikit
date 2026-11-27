//! How far each package of the manager has come: its [`State`] and whether it is attached to its services.

use crate::state::State;
use crate::steps::{sort, Steps};
use crate::{PackageId, MAX_PACKAGES};

#[derive(Clone, Copy)]
struct Slot {
    package: PackageId,
    state: State,
    /// Whether `attach` answered `Ok` for the package (and `detach` has not run since).
    attached: bool,
    /// The passes an `Attaching` package has waited for its attach to end.
    waited: u32,
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

    pub(super) fn attached(&self, p: PackageId) -> bool {
        self.find(p)
            .and_then(|i| self.slots[i])
            .is_some_and(|s| s.attached)
    }

    /// Sets the state of `p` (a new package is not attached). A graph never holds more packages than slots.
    pub(super) fn set(&mut self, p: PackageId, state: State) {
        match self.find(p) {
            Some(i) => {
                let s = self.slots[i].as_mut().unwrap();
                if s.state != state {
                    s.waited = 0;
                }
                s.state = state;
            }
            None => {
                if let Some(free) = self.slots.iter().position(|s| s.is_none()) {
                    self.slots[free] = Some(Slot {
                        package: p,
                        state,
                        attached: false,
                        waited: 0,
                    });
                }
            }
        }
    }

    pub(super) fn set_attached(&mut self, p: PackageId, on: bool) {
        if let Some(i) = self.find(p) {
            self.slots[i].as_mut().unwrap().attached = on;
        }
    }

    pub(super) fn remove(&mut self, p: PackageId) {
        if let Some(i) = self.find(p) {
            self.slots[i] = None;
        }
    }

    /// The lowest-id package whose take-down waits: a detach that answered pending or a restore that answered busy.
    pub(super) fn pending(&self) -> Option<PackageId> {
        self.slots
            .iter()
            .flatten()
            .filter(|s| s.state == State::RestorePending || s.state == State::Detaching)
            .map(|s| s.package)
            .min()
    }

    /// The packages in `Attaching`, ascending by id.
    pub(super) fn attaching(&self) -> Steps {
        let mut out = Steps::new();
        for s in self
            .slots
            .iter()
            .flatten()
            .filter(|s| s.state == State::Attaching)
        {
            out.push(s.package);
        }
        sort(&mut out);
        out
    }

    /// Counts one more pass of `p` waiting for its attach and gives the count; 0 for an absent package.
    pub(super) fn wait_one_more(&mut self, p: PackageId) -> u32 {
        match self.find(p) {
            Some(i) => {
                let s = self.slots[i].as_mut().unwrap();
                s.waited = s.waited.saturating_add(1);
                s.waited
            }
            None => 0,
        }
    }
}
