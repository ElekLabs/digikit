//! Where a reload of a service stands (`ReloadState`): the one owner of what `Rt` used to keep as two flags. Its phases are states, and the verbs are the moves between them.
//!
//! ```text
//! Idle --call--> Called --end(pending)--> Restoring --end(not pending, down)--> Down
//!                   |  \--wait_retained--> Retained --done--> Idle
//!                   \--done--> Idle                    Restoring/Down/Retained --stop_waiting / reset--> ...
//! ```
//! `Called`: the reload has its room and the take-down may begin, so the menus of the consumers are not asked any more (the closure may be down). `Retained`: the closure is
//! up again but an old copy's memory is not back (the job asks again). `Restoring`: a restore of a member waits for a task to park; the manager's pending take-down belongs to the
//! reload and the DSP step leaves it alone. `Down`: a member is down for good.
//!
//! The state blocks of a reload (`CarriedState`) live exactly while the reload is `Restoring` or going toward it. Every move that leaves the reload (`reset`, `done`, `failed`,
//! `stop_waiting`) takes the carry and the arena and frees the blocks unless it lands in `Restoring`, so no way out can keep them.

use super::carried_state::{Blocks, CarriedState};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ReloadState {
    Idle,
    Called,
    Retained,
    Restoring,
    Down,
}

impl ReloadState {
    /// A new reload starts: what an earlier one left is forgotten.
    pub(crate) unsafe fn reset(&mut self, carry: &mut CarriedState, arena: &mut impl Blocks) {
        *self = ReloadState::Idle;
        self.settle(carry, arena);
    }

    /// Every move that ends or changes the wait ends here: the blocks live on only for a restore that waits.
    unsafe fn settle(&self, carry: &mut CarriedState, arena: &mut impl Blocks) {
        if *self != ReloadState::Restoring {
            carry.release_all(arena);
        }
    }

    /// The reload has its room and goes on to the take-down. Nothing changes in a reload that already went further.
    pub(crate) fn call(&mut self) {
        if *self == ReloadState::Idle {
            *self = ReloadState::Called;
        }
    }

    /// The closure is up again and an old copy's memory is not back: the job asks again.
    pub(crate) unsafe fn wait_retained(&mut self, carry: &mut CarriedState, arena: &mut impl Blocks) {
        *self = ReloadState::Retained;
        self.settle(carry, arena);
    }

    /// The reload ended well.
    pub(crate) unsafe fn done(&mut self, carry: &mut CarriedState, arena: &mut impl Blocks) {
        *self = ReloadState::Idle;
        self.settle(carry, arena);
    }

    /// The manager ended a call with a failure. `pending`: a restore still waits. `down`: whether a member is down (`None` for a call that did not look, which keeps what is known).
    pub(crate) unsafe fn failed(&mut self, pending: bool, down: Option<bool>, carry: &mut CarriedState, arena: &mut impl Blocks) {
        let torn = match down {
            Some(d) => pending || d,
            None => self.is_torn(),
        };
        *self = match (pending, torn) {
            (true, _) => ReloadState::Restoring,
            (false, true) => ReloadState::Down,
            (false, false) => ReloadState::Idle,
        };
        self.settle(carry, arena);
    }

    /// The job stopped asking (it gave up or a member is down): the reload no longer owns a pending take-down.
    pub(crate) unsafe fn stop_waiting(&mut self, carry: &mut CarriedState, arena: &mut impl Blocks) {
        *self = match *self {
            ReloadState::Restoring => ReloadState::Down,
            ReloadState::Retained => ReloadState::Idle,
            other => other,
        };
        self.settle(carry, arena);
    }

    /// Does the reload ask to be called again (the job's next pass finishes it)? The DSP step leaves a pending take-down to it.
    pub(crate) fn is_waiting(&self) -> bool {
        matches!(self, ReloadState::Retained | ReloadState::Restoring)
    }

    /// Has the reload taken a member down?
    pub(crate) fn is_torn(&self) -> bool {
        matches!(self, ReloadState::Restoring | ReloadState::Down)
    }

    /// Is the reload past the point where the consumers' menus matter?
    pub(crate) fn is_called(&self) -> bool {
        *self != ReloadState::Idle
    }
}

#[cfg(test)]
mod tests;
