/// Where in a step an event sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Phase {
    /// The call is about to run.
    Begin = 0,
    /// The call returned; the event holds its status.
    End = 1,
    /// A point in time with no extent (a retry, a wait): it has no end to look for.
    Note = 2,
}
