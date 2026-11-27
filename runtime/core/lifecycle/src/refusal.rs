use crate::PackageId;

/// Why a graph, a bring-up or a package is refused. Names the package and the service where there is one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// More packages than [`crate::MAX_PACKAGES`].
    TooManyPackages,
    /// A package with more needs than [`crate::MAX_NEEDS`].
    TooManyNeeds { package: PackageId },
    /// Two nodes have this id.
    DuplicatePackage { package: PackageId },
    /// Two services of this name and major version.
    DuplicateService { package: PackageId, service: u32 },
    /// The header table of the package is not one the shared parser accepts, or it provides more than one service.
    BadTable { package: PackageId },
    /// A package that is not in the graph.
    UnknownPackage { package: PackageId },
    /// No package provides the service.
    Unmet { package: PackageId, service: u32 },
    /// A package provides the service, but not at a compatible version.
    Incompatible { package: PackageId, service: u32 },
    /// The needs of the packages form a cycle through this package (a package that needs a service it provides itself is one).
    Cycle { package: PackageId },
}
