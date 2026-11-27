//! Claims on pools.

use super::pool::{OwnerId, PoolKey};
use core::num::NonZeroU8;

/// A claim of `units` on a pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Want {
    pub(super) pool: PoolKey,
    pub(super) units: u32,
}

impl Want {
    pub const fn new(pool: PoolKey, units: u32) -> Want {
        Want { pool, units }
    }
}

/// One row of the claims table. The pool index and the consumer are bytes (the bounds are checked below), so a row with its `Option` tag is 8 bytes.
#[derive(Clone, Copy)]
pub(super) struct Claim {
    consumer: u8,
    /// The pool index plus one, so that `Option<Claim>` needs no tag.
    pool: NonZeroU8,
    pub(super) units: u32,
}

const _: () = assert!(super::MAX_POOLS < u8::MAX as usize);
const _: () = assert!(super::MAX_CONSUMERS <= u8::MAX as usize + 1);

impl Claim {
    pub(super) fn new(consumer: OwnerId, pool: usize, units: u32) -> Claim {
        Claim {
            consumer: consumer.0,
            pool: NonZeroU8::new(pool as u8 + 1).unwrap(),
            units,
        }
    }

    pub(super) fn consumer(&self) -> OwnerId {
        OwnerId(self.consumer)
    }

    pub(super) fn pool(&self) -> usize {
        self.pool.get() as usize - 1
    }
}
