use crate::install::Entry;
use crate::{Api, PackageId};
use dkrt_fit::ledger::{ResourceDecl, Want};

/// The data of the selected packages that the runtime reads from their files before any code runs: the header tables the graph is built from, and what
/// each header declares. The manager reads it; nothing in it changes while the manager lives.
pub trait Manifest {
    /// The header table of every package of the selection and of the services they need, by package id (`dkneeds` reads them).
    fn tables(&self) -> &[(PackageId, &[u8])];

    /// The APIs the core and the image provide without a package.
    fn core_apis(&self) -> &[Api];

    /// The pools the core declares: declared once, before any package.
    fn core_resources(&self) -> &[ResourceDecl];

    /// The pools `package` declares (a service); its `start` must declare exactly these.
    fn resources(&self, package: PackageId) -> &[ResourceDecl];

    /// What `package` claims when it comes up. A want names the pool by owner and name; a pool nobody declared cannot be claimed.
    fn wants(&self, package: PackageId) -> &[Want];

    /// The firmware entries `package` installs.
    fn entries(&self, package: PackageId) -> &[Entry];
}
