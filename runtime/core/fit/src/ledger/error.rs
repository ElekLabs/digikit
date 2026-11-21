//! Why the ledger refuses.

use super::report::PoolShort;

/// Why a declaration or a withdrawal is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerError {
    /// A pool of this owner and name exists.
    Duplicate,
    /// `draws_from` names a pool that is not declared.
    UnknownParent,
    /// No room for another pool or claim row.
    Full,
    /// The owner's pools still hold claims or feed other pools.
    InUse,
}

/// Why [`ResourceLedger::claim`] refuses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimError {
    /// A pool would overflow.
    Short(PoolShort),
    /// The claims table has no room for the new (consumer, pool) rows. Cannot happen within the bounds of [`MAX_CLAIMS`]; the check keeps a violated bound from
    /// changing anything.
    Full,
}

/// Why [`ResourceLedger::fits_with`] refuses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FitError {
    /// A pool would overflow.
    Short(PoolShort),
    /// A pending pool cannot be declared (it exists, its parent is unknown, or the table is full).
    Pool(LedgerError),
}
