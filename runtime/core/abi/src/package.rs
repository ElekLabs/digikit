//! The identity of a package, shared by the package graph (`dkrt-packages`) and the resource ledger (`dkrt-fit`). Not part of the service ABI: a service never sees it.

/// A package of the runtime: a module or a service file the loader holds. The order of ids is the order of ties in every package order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct PackageId(pub u8);
