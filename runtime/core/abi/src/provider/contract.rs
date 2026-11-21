use super::{Consumer, ConsumerId, Declarations, Status};
use crate::core_api::CoreApi;

/// One service: an owner object. The `service!` macro (RC3) holds the one instance in a static cell and passes `&mut self`; a host test constructs it
/// directly over a fake core. Optional parts are separate traits (`provider::parts`) the service adopts by name.
pub trait Provider<C: CoreApi>: Sized {
    /// Declarations the core checks against the packed header: resources with an "init" limit, entries.
    fn declare(&self, _d: &mut Declarations) {}

    /// After placement and linking. Allocate state; no consumer is attached yet.
    fn start(core: &C) -> Result<Self, Status>;

    /// A consumer came up. Its records for this service arrive here.
    fn attach(&mut self, _core: &C, _consumer: &Consumer<'_>) -> Result<(), Status> {
        Ok(())
    }

    /// A consumer goes down. Free everything it owns here: rows, objects, routes.
    fn detach(&mut self, _core: &C, _consumer: ConsumerId) {}

    /// Whether this service and its consumers may go down now (for example not while the transport plays).
    fn can_stop(&self, _core: &C) -> Result<(), Status> {
        Ok(())
    }

    /// Last call before the core restores entries and frees the package.
    fn stop(self, _core: &C) {}
}
