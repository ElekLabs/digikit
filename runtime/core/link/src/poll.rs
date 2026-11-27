/// The state of the transfer in flight.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Poll {
    Busy,
    Done,
    /// The link's error code.
    Error(u32),
}
