use crate::PackageId;
use dkrt_abi::provider::{Declarations, Status};

/// Calls into the code of packages. One seam serves services and modules: the manager runs the same steps for both (plan section 8.2), and the real
/// host tells them apart by the graph (`provides`), in one place. A service's calls are the parts of its provider row (`Provider::declare`, `start`,
/// `attach`, `detach`, `can_stop`, `stop`). A module has no provider row: its `start` runs its `init` and returns no declarations (the manager checks that
/// against its header, which has none), and `stop` runs its `fini`. `attach`, `detach` and `can_stop` name a service, so a module is only ever the
/// `consumer`: it is attached to the services it needs and never receives an `attach` itself.
///
/// Consumers are named by [`PackageId`]; the real host builds the `Consumer` (id and records) from it.
pub trait ProviderHost {
    /// Create the package's object and report what it declares. The core checks the answer against the packed header.
    fn start(&mut self, package: PackageId) -> Result<Declarations, Status>;

    /// `consumer` came up; `service` receives its records. An error refuses the consumer and leaves nothing attached.
    fn attach(&mut self, service: PackageId, consumer: PackageId) -> Result<(), Status>;

    /// `consumer` goes down: `service` frees everything it holds for it.
    fn detach(&mut self, service: PackageId, consumer: PackageId);

    /// Whether `package` may go down now (the service's `can_stop`).
    fn can_stop(&mut self, package: PackageId) -> Result<(), Status>;

    /// Last call before the entries are restored and the package is freed.
    fn stop(&mut self, package: PackageId);
}
