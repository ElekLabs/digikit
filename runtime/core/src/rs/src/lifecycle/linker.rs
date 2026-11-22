use super::session::Handle;
use crate::loader::{Images, Registry};
use dkrt_abi::package::PackageId;
use dkrt_abi::platform::Platform;
use dkrt_abi::provider::Status;
use dkrt_lifecycle::Linker;

/// Binds the imports of a placed package: the runtime's tables, the port file, and the exports of the services of its needs (`Loader::package_link`). An import that
/// resolves to the export of a service that is not among `services` is refused (`E_IMPORT`).
pub struct RtLinker<P: Platform, M: Images, R: Registry>(Handle<P, M, R>);

impl<P: Platform, M: Images, R: Registry> RtLinker<P, M, R> {
    pub(super) fn new(h: Handle<P, M, R>) -> Self {
        RtLinker(h)
    }
}

impl<P: Platform, M: Images, R: Registry> Linker for RtLinker<P, M, R> {
    fn link(&mut self, package: PackageId, _at: u32, services: &[PackageId]) -> Result<(), Status> {
        unsafe {
            let (s, l) = self.0.get();
            l.package_link(package, services, &mut s.flight)
        }
    }

    /// Nothing to undo: an import is a word of the package's own image, which `free` or `stop` releases next, and the exports a consumer was bound to belong to the service,
    /// which withdraws them when it stops (after every consumer is down).
    fn unbind(&mut self, _package: PackageId) {}
}
