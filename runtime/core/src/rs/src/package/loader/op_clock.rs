//! The number of the operation a boot or a live job runs as, for the lifecycle events (docs/INSTRUMENTATION.md "Lifecycle events").
//!
//! The clock counts the operations of one loader and says which one runs now. The operation that starts one enters it (a boot, the live job at each of its passes) and
//! leaves it at its end; the events of the manager's seams and of the installer's gate read the number from the loader they work on, so two operations never share a number
//! and no event reads another operation's.

/// The operations of one loader: how many were numbered, and the one running (0: none).
pub(crate) struct OpClock {
    last: u32,
    current: u32,
}

impl OpClock {
    pub(crate) const fn new() -> Self {
        OpClock { last: 0, current: 0 }
    }

    /// Number a new operation. It does not run until it enters.
    pub(crate) fn next(&mut self) -> u32 {
        self.last = self.last.wrapping_add(1).max(1);
        self.last
    }

    /// `op` runs now: the events of the seams carry its number.
    pub(crate) fn enter(&mut self, op: u32) {
        self.current = op;
    }

    /// The running operation ended.
    pub(crate) fn leave(&mut self) {
        self.current = 0;
    }

    /// The number of the running operation (0: none).
    pub(crate) fn current(&self) -> u32 {
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interleaved_operations_keep_their_own_numbers() {
        let mut c = OpClock::new();
        let (boot, job) = (c.next(), c.next());
        assert_ne!(boot, job);
        c.enter(boot);
        assert_eq!(c.current(), boot);
        // the job takes over a pass, then the boot's number is entered again
        c.enter(job);
        assert_eq!(c.current(), job);
        c.enter(boot);
        assert_eq!(c.current(), boot);
        c.leave();
        assert_eq!(c.current(), 0);
        // a number is never 0, and a later operation never reuses a live one
        assert_ne!(c.next(), 0);
    }
}
