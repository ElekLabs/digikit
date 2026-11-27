//! The cached summary of one other project (`dk_project_get`): the slot that was asked for last, and what its file held.
use super::super::scan::ScanResult;
use super::super::{is_busy, plan, Env};
use super::summary::ProjectSummary;
use crate::dkp::{self, Image, AREA};
use dkrt_sdk::E_NOT_FOUND;

/// After a failed read of the wanted slot (a refused or busy store), the next `get` for it waits this long before it asks again. The retry has its own clock: it never holds back the
/// work of a project.
pub const PEEK_RETRY_MS: u32 = 5000;

/// Where the cached request is.
#[repr(u8)] // zero first: the service is a bss static
#[derive(Clone, Copy, PartialEq, Eq)]
enum PeekState {
    /// Nothing was asked for, or the answer was dropped.
    Idle,
    /// `slot` was asked for: the read is queued or runs.
    Wanted,
    /// `summary` holds the file of `slot`.
    Ready,
    /// `slot` has no usable file.
    Gone,
    /// The read of `slot` failed: it is not asked again before `retry_at`.
    Backoff,
}

/// What the cache says about a slot.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PeekAnswer {
    /// Not asked for (the cache holds another slot or nothing): the caller asks.
    Other,
    Wanted,
    Ready,
    Gone,
}

/// One summary: the last slot asked for. A request for another slot replaces it.
pub struct PeekCache {
    slot: u32,
    state: PeekState,
    retry_at: u32,
    summary: ProjectSummary,
}

impl PeekCache {
    pub const fn new() -> PeekCache {
        PeekCache {
            slot: 0,
            state: PeekState::Idle,
            retry_at: 0,
            summary: ProjectSummary::new(),
        }
    }

    /// What the cache holds for `slot` at `now`. A failed read answers `Wanted` until its retry time, then `Other`. (If-let chain, not a `match`: see `Task::prepare`.)
    pub fn answer(&self, slot: u32, now: u32) -> PeekAnswer {
        if self.state == PeekState::Idle || self.slot != slot {
            PeekAnswer::Other
        } else if self.state == PeekState::Backoff {
            if (now.wrapping_sub(self.retry_at) as i32) >= 0 {
                PeekAnswer::Other
            } else {
                PeekAnswer::Wanted
            }
        } else if self.state == PeekState::Ready {
            PeekAnswer::Ready
        } else if self.state == PeekState::Gone {
            PeekAnswer::Gone
        } else {
            PeekAnswer::Wanted
        }
    }

    pub fn summary(&self) -> &ProjectSummary {
        &self.summary
    }

    /// `slot` is asked for: it replaces what the cache held.
    pub fn want(&mut self, slot: u32) {
        self.slot = slot;
        self.state = PeekState::Wanted;
    }

    /// The files of `slot` changed: an answer for it is stale.
    pub fn forget(&mut self, slot: u32) {
        if self.slot == slot && (self.state == PeekState::Ready || self.state == PeekState::Gone) {
            self.state = PeekState::Idle;
        }
    }

    /// The read of the wanted slot failed at `now` (a refused or busy store): the next `get` asks again after `PEEK_RETRY_MS`.
    pub fn give_up(&mut self, now: u32) {
        if self.state == PeekState::Wanted {
            self.state = PeekState::Backoff;
            self.retry_at = now.wrapping_add(PEEK_RETRY_MS);
        }
    }

    /// The scan for the wanted slot ended: pick the file the loader would use, read it and keep its summary. A scan for another slot than the wanted one is stale (a newer request
    /// replaced it) and changes nothing. `Err`: the store refused the read of the file with this status (the caller treats it like a refused listing).
    pub fn finish<E: Env + ?Sized>(
        &mut self,
        env: &mut E,
        buffer: &mut Image,
        result: &ScanResult,
    ) -> Result<(), i32> {
        if self.state != PeekState::Wanted || self.slot != result.slot() {
            return Ok(());
        }
        let record = result.record();
        let Some(win) = plan::pick_winner(result.files(), &record) else {
            self.state = PeekState::Gone;
            return Ok(());
        };
        let size = env.read(result.files()[win].path.as_bytes(), &mut buffer.0);
        if size == E_NOT_FOUND {
            self.state = PeekState::Gone;
            return Ok(());
        }
        if size < 0 || is_busy(size) {
            return Err(size);
        }
        let Ok(head) = dkp::check(&buffer.0[..(size as usize).min(AREA)], env.device()) else {
            self.state = PeekState::Gone;
            return Ok(());
        };
        self.summary.read_image(&buffer.0, self.slot, head.counter);
        self.state = PeekState::Ready;
        Ok(())
    }
}

impl Default for PeekCache {
    fn default() -> Self {
        Self::new()
    }
}
