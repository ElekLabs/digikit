/// What the link answered to a send.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sent {
    Accepted,
    /// Busy or ending: try again later.
    Refused,
    /// Never: an area send with a bad destination, or while the loader is retained (the answer -2 of `dl_send`).
    Hard,
}
