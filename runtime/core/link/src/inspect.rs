extern crate std;
use crate::*;
use std::vec::Vec;

impl<T: Transport> Link<T> {
    /// The live values the queue holds: `(window, word, value, not yet taken by the DSP)`, for the host tests that compare the state of the link.
    pub fn live_words(&self) -> Vec<(u32, u32, u32, bool)> {
        self.live
            .iter()
            .filter(|e| e.used)
            .map(|e| (e.win as u32, e.word, e.value, e.dirty))
            .collect()
    }

    /// The requests that are queued or in flight.
    pub fn requests_in_use(&self) -> usize {
        self.reqs.iter().filter(|r| r.state != R_FREE).count()
    }
}
