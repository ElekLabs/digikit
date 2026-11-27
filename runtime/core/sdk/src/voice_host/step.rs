use crate::{E_FORMAT, LOADING, OK};

/// The result of a call that did not fail: finished with a value, or still going (call again later with the same arguments).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step<T> {
    Done(T),
    Loading,
}

impl<T> Step<T> {
    /// Maps a status word: `OK` is done (with the value `done` makes), `LOADING` is loading, a negative code is an error.
    /// Any other positive word is not in the contract and is a format error.
    pub(crate) fn from_status(status: i32, done: impl FnOnce() -> T) -> Result<Self, i32> {
        match status {
            OK => Ok(Step::Done(done())),
            LOADING => Ok(Step::Loading),
            s if s < 0 => Err(s),
            _ => Err(E_FORMAT),
        }
    }
}
