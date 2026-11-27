//! The ledger as a check sees it: the stored pools and claims with some owners released and some wants added, without storing any of it.

use super::claim::Claim;
use super::pool::{Aggregate, OwnerId, Pool, PoolKey};
use super::report::PoolShort;
use super::{Want, MAX_CLAIMS, MAX_POOLS};

/// The consumer that [`ResourceLedger::fits`](super::ResourceLedger::fits) uses for "one new consumer": no real consumer has this id.
pub(super) const NEW_CONSUMER: OwnerId = OwnerId(u8::MAX);
const _: () = assert!(super::MAX_CONSUMERS <= u8::MAX as usize);

pub(super) struct View<'a> {
    claims: &'a [Option<Claim>; MAX_CLAIMS],
    pools: &'a [Option<Pool>; MAX_POOLS],
    released: &'a [OwnerId],
    wants: &'a [(OwnerId, &'a [Want])],
}

impl<'a> View<'a> {
    pub(super) fn new(
        claims: &'a [Option<Claim>; MAX_CLAIMS],
        pools: &'a [Option<Pool>; MAX_POOLS],
        released: &'a [OwnerId],
        wants: &'a [(OwnerId, &'a [Want])],
    ) -> View<'a> {
        View {
            claims,
            pools,
            released,
            wants,
        }
    }

    fn find(&self, key: PoolKey) -> Option<usize> {
        (0..MAX_POOLS).find(|&i| matches!(self.pools[i], Some(p) if p.key == key))
    }

    /// What `owner` would add to pool `key`.
    fn extra(&self, owner: OwnerId, key: PoolKey) -> u32 {
        let mut units = 0u32;
        for (_, ws) in self.wants.iter().filter(|(o, _)| *o == owner) {
            for w in ws.iter().filter(|w| w.pool == key) {
                units = units.saturating_add(w.units);
            }
        }
        units
    }

    /// The aggregate of the claims on pool `i`, with the wants added when `with_wants`.
    fn own(&self, i: usize, with_wants: bool) -> u32 {
        let pool = self.pools[i].unwrap();
        let mut sum = 0u32;
        let mut max = 0u32;
        let mut add = |units: u32| {
            sum = sum.saturating_add(units);
            max = max.max(units);
        };
        let held = |c: &&Claim| c.pool() == i && !self.released.contains(&c.consumer());
        for c in self.claims.iter().flatten().filter(held) {
            let extra = if with_wants {
                self.extra(c.consumer(), pool.key)
            } else {
                0
            };
            add(c.units.saturating_add(extra));
        }
        if with_wants {
            for (k, (owner, _)) in self.wants.iter().enumerate() {
                let first = !self.wants[..k].iter().any(|(o, _)| o == owner);
                let holds = self
                    .claims
                    .iter()
                    .flatten()
                    .any(|c| held(&c) && c.consumer() == *owner);
                let extra = self.extra(*owner, pool.key);
                if first && !holds && extra > 0 {
                    add(extra);
                }
            }
        }
        match pool.aggregate {
            Aggregate::Sum => sum,
            Aggregate::Max { scale } => max.saturating_mul(scale),
        }
    }

    /// The value of pool `i`: its own value plus the value of every pool that draws from it. A parent is declared before its children, so the recursion ends.
    pub(super) fn value(&self, i: usize, with_wants: bool) -> u32 {
        let mut v = self.own(i, with_wants);
        for j in 0..MAX_POOLS {
            if matches!(self.pools[j], Some(p) if p.parent == Some(i)) {
                v = v.saturating_add(self.value(j, with_wants));
            }
        }
        v
    }

    /// The same view without the last owner's wants: the state the last owner is checked against.
    fn without_last(&self) -> View<'a> {
        let n = self.wants.len().saturating_sub(1);
        View {
            claims: self.claims,
            pools: self.pools,
            released: self.released,
            wants: &self.wants[..n],
        }
    }

    /// The first pool that the wants overflow. A pool already over its limit is named only when the wants add to it. The last owner of `wants` is the one
    /// that is checked, against what is left after the owners before it (they fit already): `need` is that owner's own addition and `free` what was left.
    pub(super) fn check(&self) -> Result<(), PoolShort> {
        for (_, ws) in self.wants {
            for w in ws.iter() {
                if self.find(w.pool).is_none() {
                    return Err(PoolShort {
                        pool: w.pool,
                        need: w.units,
                        free: 0,
                    });
                }
            }
        }
        let prior = self.without_last();
        for i in 0..MAX_POOLS {
            let Some(p) = self.pools[i] else { continue };
            let before = prior.value(i, true);
            let after = self.value(i, true);
            if after > p.limit && after > before {
                return Err(PoolShort {
                    pool: p.key,
                    need: after - before,
                    free: p.limit.saturating_sub(before),
                });
            }
        }
        Ok(())
    }
}
