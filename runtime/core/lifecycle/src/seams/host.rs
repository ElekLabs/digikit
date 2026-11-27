use crate::{LiveSet, PackageGraph, PackageId};
use dkrt_abi::provider::{Declarations, Status};

/// Calls into the code of packages. One seam serves services and modules: the manager runs the same steps for both (plan section 8.2), and the real
/// host tells them apart by the graph (`provides`), in one place. A service's calls are the parts of its provider row (`Provider::declare`, `start`,
/// `attach`, `detach`, `can_stop`, `stop`). A module has no provider row: its `start` runs its `init` and returns no declarations (the manager checks that
/// against its header, which has none), and `stop` runs its `fini`. `attach` and `detach` name the consumer and the services it needs; a module is only ever the
/// `consumer` and never receives an `attach` itself.
///
/// Consumers are named by [`PackageId`]; the real host builds the `Consumer` (id and records) from it.
pub trait ProviderHost {
    /// Create the package's object and report what it declares. The core checks the answer against the packed header.
    fn start(&mut self, package: PackageId) -> Result<Declarations, Status>;

    /// Make the package public, after its entries are installed and before any consumer is attached: a service publishes its exports and resident query entry, and
    /// the package's tick and listing client start. Nothing of the package is visible to others before this call. An error leaves nothing public.
    fn activate(&mut self, package: PackageId) -> Result<(), Status>;

    /// Undo `activate`, before the entries are restored: the tick stops and the resident entry points are withdrawn. Repeating it is harmless.
    fn deactivate(&mut self, package: PackageId);

    /// The place of `service` in the order its callbacks run (the rows' walk order): a consumer is attached to the services it needs in ascending rank, ties by id, and detached
    /// in the reverse. The default is the package id.
    fn attach_rank(&self, service: PackageId) -> u32 {
        service.0 as u32
    }

    /// `consumer` came up, once, after every one of its `services` is up (in the order of [`attach_rank`](ProviderHost::attach_rank), ties by id; none for a package that
    /// needs no service): the services receive its records. The host runs the whole walk, so that a record's place among the others in the file is kept across services (the
    /// rows the services share are numbered in that order). An error refuses the consumer and leaves nothing attached: the host undoes what the walk did.
    fn attach(&mut self, consumer: PackageId, services: &[PackageId]) -> Result<(), Status>;

    /// `consumer` goes down: its `services` free everything they hold for it (the walk of `attach` backwards). Called for a consumer that `attach` answered `Ok` for; repeating it
    /// is harmless.
    fn detach(&mut self, consumer: PackageId, services: &[PackageId]);

    /// Whether `package` may go down now (the service's `can_stop`).
    fn can_stop(&mut self, package: PackageId) -> Result<(), Status>;

    /// A reload of `package` is about to stop it: its entries are restored and nothing enters the service any more, so its state is final. The host takes the
    /// service's state aside for the new copy (`ProviderHost::import_state`). Called only for a reload, once for each package of the reload's take-down (also after
    /// a busy restore was retried), never for an unload or the take-down of a failed bring-up. The default does nothing.
    fn export_state(&mut self, _package: PackageId) {}

    /// `package` has started and its declarations match, before its entries are installed: the host hands it the state `export_state` took aside, or none (a new
    /// start, or bytes it cannot take). An error fails the start like a failed `start`: the closure goes down and nothing is public, and the copy is stopped and freed. `E_BUSY` is not retried: a copy that has just started has no work in flight, so a busy answer is a failure like any other. The default does nothing.
    fn import_state(&mut self, _package: PackageId) -> Result<(), Status> {
        Ok(())
    }

    /// Last call before the entries are restored and the package is freed.
    fn stop(&mut self, package: PackageId);

    /// A boot has tried every service and is about to bring up the first module (or has no module to bring up): `live` holds the services that are up. The host
    /// plans what the services keep for each module that can still come up, which is one whose services are all in `live`. Called once per boot and once per reload (the
    /// services that came back have fresh state; a reload numbers again before its first consumer starts), never for a load.
    fn services_up(&mut self, _graph: &PackageGraph, _live: &LiveSet) {}

    /// `package` is `Active`: start its DSP part, if it has one. The DSP part comes up over later UI passes, so this call returns at once: `true` when a DSP part is
    /// coming (the package counts in [`Report::dsp_pending`](crate::Report::dsp_pending)), `false` for a package with none. The host reports the end of a DSP part that failed with
    /// [`PackageManager::take_down_closure`](crate::PackageManager::take_down_closure); one that succeeded needs no call. Called once for each package a bring-up brought up,
    /// right after it is `Active` and before the next package of the bring-up is placed (the DSP loader reserves for one package at a time): it never blocks a boot.
    fn start_dsp(&mut self, _package: PackageId) -> bool {
        false
    }
}
