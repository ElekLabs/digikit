//! The clock of a busy store: when to try again, when to give up for now, and whether a deferral was logged. UI task only.
use dkrt_sdk::proto::E_FILE_IN_USE;
use dkrt_sdk::E_BUSY;

/// The tick tries a busy store again after this long.
pub const RETRY_MS: u32 = 1000;
/// After this long of a busy store the work gives up for now: one `PROJECT_DEFERRED`; the image stays dirty and the next save writes.
pub const DEFER_MS: u32 = 30_000;

/// Is `status` an answer of a store that is busy now and may be free later (`E_BUSY`, `E_FILE_IN_USE`)? Any other status is final for the attempt.
pub fn is_busy(status: i32) -> bool {
    status == E_BUSY || status == E_FILE_IN_USE
}

/// What a busy store makes the tick do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BusyVerdict {
    /// Try the same thing again after `RETRY_MS`.
    Retry,
    /// The store was busy for `waited` ms: give up for now. `first` is true only the first time since a write succeeded.
    GiveUp { waited: u32, first: bool },
}

/// Whether the tick waits before it touches the store again.
#[derive(Clone, Copy)]
#[repr(u8)] // an explicit tag, so that `Free` is all zero (the service is a bss static)
enum Hold {
    Free,
    /// Wait until this time (ms, wraps).
    Until(u32),
}

/// Owns the retry hold, the time the store first answered busy, and the deferred flag (`dk_project_status` bit 3). Held by `ProjectService`; the scan and the write job both
/// report their busy answers to it.
pub struct BusyClock {
    hold: Hold,
    /// When the store began to be busy (`None`: not busy now).
    since: Option<u32>,
    /// A deferral was logged and no write succeeded since.
    deferred: bool,
}

impl BusyClock {
    pub const fn new() -> BusyClock {
        BusyClock {
            hold: Hold::Free,
            since: None,
            deferred: false,
        }
    }

    /// The tick waits for the retry time of a busy store.
    pub fn must_wait(&self, now: u32) -> bool {
        if let Hold::Until(at) = self.hold {
            return (now.wrapping_sub(at) as i32) < 0;
        }
        false
    }

    /// The tick waits `ms` before it tries again (a full job table, a scan the store refused).
    pub fn hold_for(&mut self, now: u32, ms: u32) {
        self.hold = Hold::Until(now.wrapping_add(ms));
    }

    /// The store answered: no wait is pending and the busy time starts over.
    pub fn answered(&mut self) {
        self.hold = Hold::Free;
        self.since = None;
    }

    /// The store was busy: retry after `RETRY_MS`, or give up after `DEFER_MS` of busy answers in a row.
    pub fn note_busy(&mut self, now: u32) -> BusyVerdict {
        let since = *self.since.get_or_insert(now);
        let waited = now.wrapping_sub(since);
        if waited >= DEFER_MS {
            let first = !self.deferred;
            self.deferred = true;
            return BusyVerdict::GiveUp { waited, first };
        }
        self.hold_for(now, RETRY_MS);
        BusyVerdict::Retry
    }

    /// A deferral was logged and no write succeeded since.
    pub fn is_deferred(&self) -> bool {
        self.deferred
    }

    /// The deferred flag of a copy this one replaces.
    pub(super) fn restore_deferred(&mut self, deferred: bool) {
        self.deferred = deferred;
    }

    /// A write succeeded: the store is no longer deferred.
    pub fn clear_deferred(&mut self) {
        self.deferred = false;
    }
}

impl Default for BusyClock {
    fn default() -> Self {
        Self::new()
    }
}
