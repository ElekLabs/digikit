//! The `what` word of `PROJECT_MODULE`.

/// The `what` word of `PROJECT_MODULE`.
pub struct Event;

impl Event {
    /// The live load was queued.
    pub const QUEUED: u32 = 1;
    /// The load ended well; the status is the result of adding the module to the saved selection.
    pub const DONE: u32 = 2;
    /// The module is not on the card.
    pub const MISSING: u32 = 3;
    /// The runtime refused the load, or it failed; the status is the refusal.
    pub const REFUSED: u32 = 4;
    /// The module is loaded but cannot be kept in the saved selection; the status is why.
    pub const NOT_KEPT: u32 = 5;
}
