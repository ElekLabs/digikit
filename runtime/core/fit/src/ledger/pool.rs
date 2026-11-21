//! Pools: who declares them, how their claims add up.

/// The id of a pool owner or consumer: a package of the runtime (the same byte as `dkrt_abi::package::PackageId`; the lifecycle converts at its edge).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct OwnerId(pub u8);

/// Who declared a pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    Core,
    Package(OwnerId),
}

/// A pool: its owner and the hash of its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoolKey {
    pub(super) owner: Owner,
    pub(super) name: u32,
}

impl PoolKey {
    pub const fn new(owner: Owner, name: u32) -> PoolKey {
        PoolKey { owner, name }
    }

    pub const fn owner(&self) -> Owner {
        self.owner
    }

    pub const fn name(&self) -> u32 {
        self.name
    }
}

/// How the claims on a pool add up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aggregate {
    /// The sum of the claims.
    Sum,
    /// The largest claim of one consumer times `scale`: every unit (a voice) may run any consumer's claim, so the largest counts for all units.
    Max { scale: u32 },
}

/// A resource a package declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceDecl {
    pub(super) name: u32,
    pub(super) limit: u32,
    pub(super) aggregate: Aggregate,
    /// The pool whose limit this pool's value also counts against (it must be declared already).
    pub(super) draws_from: Option<PoolKey>,
}

impl ResourceDecl {
    /// A pool that draws from no other.
    pub const fn new(name: u32, limit: u32, aggregate: Aggregate) -> ResourceDecl {
        ResourceDecl {
            name,
            limit,
            aggregate,
            draws_from: None,
        }
    }

    /// This pool's value also counts against `parent`'s limit (the parent must be declared already).
    pub const fn drawing_from(mut self, parent: PoolKey) -> ResourceDecl {
        self.draws_from = Some(parent);
        self
    }
}

#[derive(Clone, Copy)]
pub(super) struct Pool {
    pub(super) key: PoolKey,
    pub(super) limit: u32,
    pub(super) aggregate: Aggregate,
    pub(super) parent: Option<usize>,
}
