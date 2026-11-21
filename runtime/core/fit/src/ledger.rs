//! The resource ledger (docs/plans/runtime-contract-and-lifecycle.md, sections 6.5 and 7): counted capacity that the core and the services declare and that
//! consumers claim. A pool is `(owner, name hash)` with a limit, an aggregate and an optional parent it draws from; the ledger sums the claims and admits
//! or refuses them. It knows no resource by meaning: the DSP voice rule is two pools drawing from one (`Max` for the voices, `Sum` for the effects).
//!
//! Pure and fixed-size, like the rest of this crate. The planner asks [`ResourceLedger::fits`] from header tables before any code runs; the package
//! manager (RC2) claims on bring-up and releases on take-down. Nothing in the runtime uses it yet. The device-facing fit rule in `lib.rs` is unchanged.

use dkrt_abi::package::PackageId;
use dkrt_abi::REC_MAX;
use dkrt_sdk::CAP_MAX;

/// Most pools: the CAPACITY reply has room for this many entries.
pub const MAX_POOLS: usize = CAP_MAX;
/// Most claims: one row per (consumer, pool) pair. A consumer is a package (at most `REC_MAX`, the loader's records) and claims each pool at most once (rows
/// of one pair add up), so no consumer holds more rows than there are pools (`MAX_POOLS`).
pub const MAX_CLAIMS: usize = REC_MAX * MAX_POOLS;

mod claim;
mod error;
mod pool;
mod report;

pub use claim::Want;
pub use error::{ClaimError, LedgerError};
pub use pool::{Aggregate, Owner, PoolKey, ResourceDecl};
pub use report::{PoolReport, PoolShort};

use claim::Claim;
use pool::Pool;

/// The pools and the claims on them.
pub struct ResourceLedger {
    pools: [Option<Pool>; MAX_POOLS],
    claims: [Option<Claim>; MAX_CLAIMS],
}

impl Default for ResourceLedger {
    fn default() -> Self {
        ResourceLedger::new()
    }
}

impl ResourceLedger {
    pub const fn new() -> ResourceLedger {
        ResourceLedger {
            pools: [None; MAX_POOLS],
            claims: [None; MAX_CLAIMS],
        }
    }

    fn find(&self, key: PoolKey) -> Option<usize> {
        (0..MAX_POOLS).find(|&i| matches!(self.pools[i], Some(p) if p.key == key))
    }

    /// Declares a pool of `owner` (`Owner::Core` or a package). Refuses a pool that exists and a parent that is not declared.
    pub fn declare(&mut self, owner: Owner, r: &ResourceDecl) -> Result<(), LedgerError> {
        let key = PoolKey {
            owner,
            name: r.name,
        };
        if self.find(key).is_some() {
            return Err(LedgerError::Duplicate);
        }
        let parent = match r.draws_from {
            Some(k) => Some(self.find(k).ok_or(LedgerError::UnknownParent)?),
            None => None,
        };
        let slot = (0..MAX_POOLS)
            .find(|&i| self.pools[i].is_none())
            .ok_or(LedgerError::Full)?;
        self.pools[slot] = Some(Pool {
            key,
            limit: r.limit,
            aggregate: r.aggregate,
            parent,
        });
        Ok(())
    }

    /// The claim of `consumer` on pool `i`, with `extra` units of `wants` added when `extra_for` is that consumer (or a new consumer when it has no claim).
    fn own(&self, i: usize, wants: &[Want], extra_for: Option<PackageId>) -> u32 {
        let pool = self.pools[i].unwrap();
        let mut extra = 0u32;
        for w in wants {
            if w.pool == pool.key {
                extra = extra.saturating_add(w.units);
            }
        }
        let mut sum = 0u32;
        let mut max = 0u32;
        let mut extra_placed = extra == 0;
        for c in self.claims.iter().flatten().filter(|c| c.pool() == i) {
            let mut units = c.units;
            if !extra_placed && Some(c.consumer()) == extra_for {
                units = units.saturating_add(extra);
                extra_placed = true;
            }
            sum = sum.saturating_add(units);
            max = max.max(units);
        }
        if !extra_placed {
            sum = sum.saturating_add(extra);
            max = max.max(extra);
        }
        match pool.aggregate {
            Aggregate::Sum => sum,
            Aggregate::Max { scale } => max.saturating_mul(scale),
        }
    }

    /// The value of pool `i` with the wants applied: its own value plus the value of every pool that draws from it. A parent is declared before its children,
    /// so the recursion ends.
    fn value(&self, i: usize, wants: &[Want], extra_for: Option<PackageId>) -> u32 {
        let mut v = self.own(i, wants, extra_for);
        for j in 0..MAX_POOLS {
            if matches!(self.pools[j], Some(p) if p.parent == Some(i)) {
                v = v.saturating_add(self.value(j, wants, extra_for));
            }
        }
        v
    }

    fn check(&self, wants: &[Want], extra_for: Option<PackageId>) -> Result<(), PoolShort> {
        for w in wants {
            if self.find(w.pool).is_none() {
                return Err(PoolShort {
                    pool: w.pool,
                    need: w.units,
                    free: 0,
                });
            }
        }
        for i in 0..MAX_POOLS {
            let Some(p) = self.pools[i] else { continue };
            let before = self.value(i, &[], None);
            let after = self.value(i, wants, extra_for);
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

    /// Whether `wants`, claimed by one new consumer, fit. Changes nothing. The first overflowing pool in declaration order is named.
    pub fn fits(&self, wants: &[Want]) -> Result<(), PoolShort> {
        self.check(wants, None)
    }

    /// Claims `wants` for `consumer`, all or nothing. A consumer that claims a pool again adds to its claim.
    pub fn claim(&mut self, consumer: PackageId, wants: &[Want]) -> Result<(), ClaimError> {
        self.check(wants, Some(consumer))
            .map_err(ClaimError::Short)?;
        // room for the new (consumer, pool) rows first, so that a full table changes nothing
        let mut fresh = 0;
        for (k, w) in wants.iter().enumerate() {
            let i = self.find(w.pool).unwrap();
            let known = self
                .claims
                .iter()
                .flatten()
                .any(|c| c.consumer() == consumer && c.pool() == i);
            let earlier = wants[..k].iter().any(|e| e.pool == w.pool);
            if !known && !earlier {
                fresh += 1;
            }
        }
        if self.claims.iter().filter(|c| c.is_none()).count() < fresh {
            return Err(ClaimError::Full);
        }
        for w in wants {
            let i = self.find(w.pool).unwrap();
            let existing = self
                .claims
                .iter_mut()
                .flatten()
                .find(|c| c.consumer() == consumer && c.pool() == i);
            match existing {
                Some(c) => c.units = c.units.saturating_add(w.units),
                None => {
                    let slot = self.claims.iter().position(|c| c.is_none()).unwrap();
                    self.claims[slot] = Some(Claim::new(consumer, i, w.units));
                }
            }
        }
        Ok(())
    }

    /// Returns every unit `consumer` claimed.
    pub fn release(&mut self, consumer: PackageId) {
        for c in self.claims.iter_mut() {
            if matches!(c, Some(x) if x.consumer() == consumer) {
                *c = None;
            }
        }
    }

    /// Removes every pool of `owner`. Refused while a claim names one of them or another owner's pool draws from one.
    pub fn withdraw(&mut self, owner: Owner) -> Result<(), LedgerError> {
        let mine = |s: &Self, i: usize| matches!(s.pools[i], Some(p) if p.key.owner == owner);
        for i in 0..MAX_POOLS {
            if !mine(self, i) {
                continue;
            }
            if self.claims.iter().flatten().any(|c| c.pool() == i) {
                return Err(LedgerError::InUse);
            }
            if (0..MAX_POOLS)
                .any(|j| !mine(self, j) && matches!(self.pools[j], Some(p) if p.parent == Some(i)))
            {
                return Err(LedgerError::InUse);
            }
        }
        for i in 0..MAX_POOLS {
            if mine(self, i) {
                self.pools[i] = None;
            }
        }
        Ok(())
    }

    /// Writes one row per pool, in declaration slots, into `out`; the number of rows (at most `out.len()`).
    pub fn report(&self, out: &mut [PoolReport]) -> usize {
        let mut n = 0;
        for i in 0..MAX_POOLS {
            let Some(p) = self.pools[i] else { continue };
            if n == out.len() {
                break;
            }
            let used = self.value(i, &[], None);
            let largest = match p.aggregate {
                Aggregate::Sum => p.limit.saturating_sub(used),
                Aggregate::Max { .. } => self
                    .claims
                    .iter()
                    .flatten()
                    .filter(|c| c.pool() == i)
                    .map(|c| c.units)
                    .max()
                    .unwrap_or(0),
            };
            out[n] = PoolReport {
                pool: p.key,
                limit: p.limit,
                used,
                largest,
            };
            n += 1;
        }
        n
    }
}

#[cfg(test)]
mod tests;
