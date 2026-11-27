use crate::PackageId;
use dkrt_abi::provider::Status;

/// Binds the imports of a placed package to the services it needs.
pub trait Linker {
    /// Link `package` at `at`. `services` are the live services of its needs (the same data as the graph edges): the linker refuses an import of a
    /// service that is not among them. On an error nothing stays bound.
    fn link(&mut self, package: PackageId, at: u32, services: &[PackageId]) -> Result<(), Status>;

    /// Undo `link`: no import of `package` points into a service any more. Called once for each package that was linked.
    fn unbind(&mut self, package: PackageId);
}
