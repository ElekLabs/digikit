//! The edge to the ledger: package ids become owner ids here, and the claims of a bring-up are planned before anything is placed.

use super::bring_up::refused_need;
use super::refused::RefusedList;
use super::{LifecycleError, PackageManager};
use crate::seams::{Linker, Manifest, PackageInstaller, Placer, ProviderHost};
use crate::steps::Steps;
use crate::{PackageId, MAX_PACKAGES};
use dkrt_abi::provider::{Declarations, Limit};
use dkrt_fit::ledger::{
    Aggregate, ClaimError, FitError, LedgerError, Owner, OwnerId, ResourceDecl, Want, MAX_POOLS,
};

// A package id is a ledger consumer: the ledger must have a row for every record the loader can hold.
const _: () = assert!(dkrt_fit::ledger::MAX_CONSUMERS == dkrt_abi::REC_MAX);

pub(super) fn owner_id(p: PackageId) -> OwnerId {
    OwnerId(p.0)
}

impl<M, P, L, I, H> PackageManager<M, P, L, I, H>
where
    M: Manifest,
    P: Placer,
    L: Linker,
    I: PackageInstaller,
    H: ProviderHost,
{
    /// Declares the pools of `p`'s header.
    pub(super) fn declare_resources(&mut self, p: PackageId) -> Result<(), LifecycleError> {
        for r in self.manifest.resources(p) {
            self.ledger
                .declare(Owner::Package(owner_id(p)), r)
                .map_err(|error| LifecycleError::Ledger { package: p, error })?;
        }
        Ok(())
    }

    /// Claims `p`'s wants.
    pub(super) fn claim_wants(&mut self, p: PackageId) -> Result<(), LifecycleError> {
        self.ledger
            .claim(owner_id(p), self.manifest.wants(p))
            .map_err(|e| match e {
                ClaimError::Short(short) => LifecycleError::Short { package: p, short },
                ClaimError::Full => LifecycleError::Ledger {
                    package: p,
                    error: dkrt_fit::ledger::LedgerError::Full,
                },
            })
    }

    /// Returns the claims of `p` and removes its pools. The graph takes a service down after its consumers, so no claim names its pools any more.
    pub(super) fn release_resources(&mut self, p: PackageId) {
        self.ledger.release(owner_id(p));
        let _ = self.ledger.withdraw(Owner::Package(owner_id(p)));
    }

    /// Whether what `p`'s `start` declared is what its header says: the same pools, the same fixed limits. `Init` limits are decided by the service after
    /// `start`, so only the name is compared.
    pub(super) fn declaration_matches(&self, p: PackageId, d: &Declarations) -> bool {
        let header = self.manifest.resources(p);
        d.resources().len() == header.len()
            && header.iter().all(|r| {
                d.resources().iter().any(|&(name, limit)| match limit {
                    Limit::Fixed(n) => name == r.name() && n == r.limit(),
                    Limit::Init => name == r.name(),
                })
            })
    }

    /// The fit plan of `order`, package by package in bring-up order, without touching the ledger: each package's pools and wants are added to those of the
    /// packages accepted before it and checked together ([`ResourceLedger::fits_with`]; the `released` owners' pools and claims count as returned). A
    /// package that does not fit is refused, and so are the packages that need a refused service. Of the rest, a service stays in the plan only if a
    /// root or a package that stays needs it: nothing is placed for a service whose consumers were all refused. Returns the packages to bring up, in
    /// order. A shortage names the package that tipped it; its `need` and `free` count the claims of all accepted packages together.
    pub(super) fn plan_fit(
        &self,
        order: &Steps,
        roots: &[PackageId],
        released: &[OwnerId],
        refused: &mut RefusedList,
    ) -> Steps {
        const FILLER: ResourceDecl = ResourceDecl::new(0, 0, Aggregate::Sum);
        let mut pending = [(Owner::Core, &FILLER); MAX_POOLS];
        let mut pools = 0;
        let mut wants: [(OwnerId, &[Want]); MAX_PACKAGES] = [(OwnerId(0), &[]); MAX_PACKAGES];
        let mut owners = 0;
        let mut fitted = Steps::new();
        for &p in order.as_slice() {
            if let Some(service) = refused_need(&self.graph, p, refused) {
                refused.push(
                    p,
                    LifecycleError::NeedRefused {
                        package: p,
                        service,
                    },
                );
                continue;
            }
            let own_pools = self.manifest.resources(p);
            if pools + own_pools.len() > MAX_POOLS {
                refused.push(
                    p,
                    LifecycleError::Ledger {
                        package: p,
                        error: LedgerError::Full,
                    },
                );
                continue;
            }
            for r in own_pools {
                pending[pools] = (Owner::Package(owner_id(p)), r);
                pools += 1;
            }
            wants[owners] = (owner_id(p), self.manifest.wants(p));
            owners += 1;
            match self
                .ledger
                .fits_with(&pending[..pools], released, &wants[..owners])
            {
                Ok(()) => fitted.push(p),
                Err(e) => {
                    pools -= own_pools.len();
                    owners -= 1;
                    refused.push(
                        p,
                        match e {
                            FitError::Short(short) => LifecycleError::Short { package: p, short },
                            FitError::Pool(error) => LifecycleError::Ledger { package: p, error },
                        },
                    );
                }
            }
        }
        let mut stays = Steps::new();
        for &p in fitted.as_slice().iter().rev() {
            if roots.contains(&p) || self.graph.consumers_of(p).any(|c| stays.contains(c)) {
                stays.push(p);
            }
        }
        let mut plan = Steps::new();
        for &p in fitted.as_slice() {
            if stays.contains(p) {
                plan.push(p);
            }
        }
        plan
    }
}
