//! The resource ledger (docs/plans/runtime-contract-and-lifecycle.md, sections 6.5 and 7): counted capacity that the core and the services declare and that
//! consumers claim. A pool is `(owner, name hash)` with a limit, an aggregate and an optional parent it draws from; the ledger sums the claims and admits
//! or refuses them. It knows no resource by meaning: the DSP voice rule is two pools drawing from one (`Max` for the voices, `Sum` for the effects).
//!
//! Pure and fixed-size, like the rest of this crate. The planner asks [`ResourceLedger::fits`] from header tables before any code runs; the package
//! manager (RC2) claims on bring-up and releases on take-down. Nothing in the runtime uses it yet. The device-facing fit rule in `lib.rs` is unchanged.

use core::ptr::addr_of_mut;
use dkrt_sdk::{CAP_MAX, PROVIDER_MAX, SEL_MAX};

/// Most pools: the CAPACITY reply has room for this many entries.
pub const MAX_POOLS: usize = CAP_MAX;
/// Most consumers: the loader's records (`dkrt_abi::REC_MAX`: the selectable modules plus the providers).
pub const MAX_CONSUMERS: usize = SEL_MAX + PROVIDER_MAX;
/// Most claims: one row per (consumer, pool) pair. A consumer is a package (at most `MAX_CONSUMERS`, the loader's records) and claims each pool at most once (rows
/// of one pair add up), so no consumer holds more rows than there are pools (`MAX_POOLS`).
pub const MAX_CLAIMS: usize = MAX_CONSUMERS * MAX_POOLS;

mod claim;
mod error;
mod pool;
mod report;
mod view;

pub use claim::Want;
pub use error::{ClaimError, FitError, LedgerError};
pub use pool::{Aggregate, Owner, OwnerId, PoolKey, ResourceDecl};
pub use report::{PoolReport, PoolShort};

use claim::Claim;
use pool::Pool;
use view::{View, NEW_CONSUMER};

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

    /// An empty ledger at `out`, with no copy of it on the stack (a ledger is 2.4 KB).
    ///
    /// # Safety
    /// `out` is valid for writes of a ledger.
    pub unsafe fn init(out: *mut ResourceLedger) {
        // slot by slot: a copy of a constant would be built on the stack first
        let pools = addr_of_mut!((*out).pools).cast::<Option<Pool>>();
        for i in 0..MAX_POOLS {
            pools.add(i).write(None);
        }
        let claims = addr_of_mut!((*out).claims).cast::<Option<Claim>>();
        for i in 0..MAX_CLAIMS {
            claims.add(i).write(None);
        }
    }

    fn find(&self, key: PoolKey) -> Option<usize> {
        (0..MAX_POOLS).find(|&i| matches!(self.pools[i], Some(p) if p.key == key))
    }

    /// Declares a pool of `owner` (`Owner::Core` or a package). Refuses a pool that exists and a parent that is not declared.
    pub fn declare(&mut self, owner: Owner, r: &ResourceDecl) -> Result<(), LedgerError> {
        declare_in(&mut self.pools, owner, r)
    }

    fn view<'a>(
        &'a self,
        pools: &'a [Option<Pool>; MAX_POOLS],
        released: &'a [OwnerId],
        wants: &'a [(OwnerId, &'a [Want])],
    ) -> View<'a> {
        View::new(&self.claims, pools, released, wants)
    }

    /// Whether `wants`, claimed by one new consumer, fit. Changes nothing. The first overflowing pool in declaration order is named.
    pub fn fits(&self, wants: &[Want]) -> Result<(), PoolShort> {
        self.view(&self.pools, &[], &[(NEW_CONSUMER, wants)])
            .check()
    }

    /// Whether a bring-up fits, decided without changing anything: `pending` pools would be declared, the pools and claims of the `released` owners
    /// would be gone, and each owner of `wants` would claim its list (on top of what it holds, if it is not released). The first overflowing pool in
    /// declaration order is named; a pool that a want names and nobody declares is short with nothing free. A pending pool that exists, names an unknown
    /// parent or finds no slot is a [`FitError::Pool`]. Each owner appears at most once in `wants`; a released owner's pools are drawn from only by pools
    /// of released owners.
    pub fn fits_with(
        &self,
        pending: &[(Owner, &ResourceDecl)],
        released: &[OwnerId],
        wants: &[(OwnerId, &[Want])],
    ) -> Result<(), FitError> {
        let mut pools = self.pools;
        for slot in pools.iter_mut() {
            if matches!(slot, Some(p) if matches!(p.key.owner, Owner::Package(o) if released.contains(&o)))
            {
                *slot = None;
            }
        }
        for (owner, r) in pending {
            declare_in(&mut pools, *owner, r).map_err(FitError::Pool)?;
        }
        self.view(&pools, released, wants)
            .check()
            .map_err(FitError::Short)
    }

    /// Claims `wants` for `consumer`, all or nothing. A consumer that claims a pool again adds to its claim.
    pub fn claim(&mut self, consumer: OwnerId, wants: &[Want]) -> Result<(), ClaimError> {
        self.view(&self.pools, &[], &[(consumer, wants)])
            .check()
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
    pub fn release(&mut self, consumer: OwnerId) {
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
            let used = self.view(&self.pools, &[], &[]).value(i, false);
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

/// Declares `r` for `owner` in `pools`: the one rule of [`ResourceLedger::declare`] and of the trial of [`ResourceLedger::fits_with`].
fn declare_in(
    pools: &mut [Option<Pool>; MAX_POOLS],
    owner: Owner,
    r: &ResourceDecl,
) -> Result<(), LedgerError> {
    let find = |pools: &[Option<Pool>; MAX_POOLS], key: PoolKey| {
        (0..MAX_POOLS).find(|&i| matches!(pools[i], Some(p) if p.key == key))
    };
    let key = PoolKey {
        owner,
        name: r.name,
    };
    if find(pools, key).is_some() {
        return Err(LedgerError::Duplicate);
    }
    let parent = match r.draws_from {
        Some(k) => Some(find(pools, k).ok_or(LedgerError::UnknownParent)?),
        None => None,
    };
    let slot = (0..MAX_POOLS)
        .find(|&i| pools[i].is_none())
        .ok_or(LedgerError::Full)?;
    pools[slot] = Some(Pool {
        key,
        limit: r.limit,
        aggregate: r.aggregate,
        parent,
    });
    Ok(())
}

#[cfg(test)]
mod tests;
