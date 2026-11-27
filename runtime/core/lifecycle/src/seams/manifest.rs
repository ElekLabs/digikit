use crate::install::Entry;
use crate::{Api, PackageId};
use dkrt_abi::provider::Status;
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

    /// The rules the core applies on top of the graph and the ledger: one owner for each kind, slot and hook site, no key twice, the uses within the capacity of the
    /// device. Asked once for each package of a bring-up, in bring-up order, after the graph accepted it and its services were admitted and before anything of it is
    /// placed; a package that is admitted takes its share of the capacity, so later packages see it. A refusal names its status, refuses the package with the packages
    /// that need it and takes nothing. The default admits everything.
    fn admit(&mut self, _package: PackageId) -> Result<(), Status> {
        Ok(())
    }
}
