//! What the ledger reports about its pools.

use super::pool::{Owner, PoolKey};

/// The first pool that a set of wants overflows: what the wants add to it and what it had free (as `fits_now` of the device rule reports a short resource).
/// For a pool that the wants name but nobody declared, `free` is 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoolShort {
    pub(super) pool: PoolKey,
    pub(super) need: u32,
    pub(super) free: u32,
}

impl PoolShort {
    pub const fn pool(&self) -> PoolKey {
        self.pool
    }

    pub const fn need(&self) -> u32 {
        self.need
    }

    pub const fn free(&self) -> u32 {
        self.free
    }
}

/// One row of [`ResourceLedger::report`]: the CAPACITY entry of a pool. `used` is the pool's value including the pools that draw from it. `largest` is the
/// largest single claim for a `Max` pool and the free amount for a `Sum` pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoolReport {
    pub(super) pool: PoolKey,
    pub(super) limit: u32,
    pub(super) used: u32,
    pub(super) largest: u32,
}

impl PoolReport {
    pub const fn pool(&self) -> PoolKey {
        self.pool
    }

    pub const fn limit(&self) -> u32 {
        self.limit
    }

    pub const fn used(&self) -> u32 {
        self.used
    }

    pub const fn largest(&self) -> u32 {
        self.largest
    }

    /// An empty row, to size the buffer of [`ResourceLedger::report`].
    pub const EMPTY: PoolReport = PoolReport {
        pool: PoolKey {
            owner: Owner::Core,
            name: 0,
        },
        limit: 0,
        used: 0,
        largest: 0,
    };
}
