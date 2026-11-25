use super::{Consumer, ConsumerId, Declarations, Status};
use crate::core_api::CoreApi;
use crate::service::SettleCtx;

/// One service: an owner object. The `service!` macro holds the one instance in a cell (`Owner<T>` of `dkrt-svc-rt`), starts it in the file's `init` and stops it in
/// `fini`; a host test constructs it directly over a fake core. Optional parts are separate traits (`provider::parts`) the service adopts by name.
///
/// This is the lifecycle part (tag 1, version 1; table `tables::LifecycleV1`). `start` and `stop` are not in the table: the file's own entry points run them.
pub trait Provider<C: CoreApi>: Sized {
    /// Declarations the core checks against the packed header: resources with an "init" limit, entries.
    fn declare(&self, _d: &mut Declarations) {}

    /// The file's `init`, before the declarations. Allocate state; no consumer is attached yet. The value is returned by move: a service whose state points into itself
    /// (strings, rows) must not be moved after it is built, so it starts in `start_in` and refuses here.
    fn start(core: &C) -> Result<Self, Status>;

    /// What the file's `init` calls: `start` into the slot. The default moves the value `start` returns into the slot; a service with a large state overrides it and writes
    /// the state where it lives (no copy through the stack). Started state must not move afterwards: self-pointers made here stay valid until `stop`. It must leave nothing that needs a drop when it fails.
    fn start_in(core: &C, slot: &mut core::mem::MaybeUninit<Self>) -> Result<(), Status> {
        slot.write(Self::start(core)?);
        Ok(())
    }

    /// A consumer came up: every provider it needs hears it once, in table order, also when the module has no records. The records of the service's kinds follow, one
    /// `Records::apply` each. A refusal is undone with `detach` for the same consumer.
    fn attach(&mut self, _core: &C, _consumer: &Consumer<'_>) -> Result<(), Status> {
        Ok(())
    }

    /// A consumer goes down (also one that was not attached, or only in part: it is idempotent). Free everything it owns here: rows, objects, routes.
    fn detach(&mut self, _core: &C, _consumer: ConsumerId) {}

    /// Whether this service and its consumers may go down now (for example not while the transport plays).
    fn can_stop(&self, _core: &C) -> Result<(), Status> {
        Ok(())
    }

    /// The file's `fini`: last call before the core restores entries and frees the package. The instance is dropped where it lives right after (it is not moved).
    fn stop(&mut self, _core: &C) {}

    /// The boot has loaded every service its selection needs and loads no module yet; `s` has the keys of each selected module. Once per boot. A service that keeps
    /// state across boots plans and saves it here.
    fn settle(&mut self, _core: &C, _s: &SettleCtx) {}

    /// A reload of a module ended, with the new version loaded or not: give back what this service kept for the module during the reload that the new version did not
    /// take over. Once per reload.
    fn reload_done(&mut self, _core: &C) {}
}
