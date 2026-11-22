use super::session::Handle;
use crate::handlers::services;
use crate::loader::{Images, Registry};
use dkrt_abi::package::PackageId;
use dkrt_abi::platform::Platform;
use dkrt_abi::provider::{Declarations, Status};
use dkrt_abi::service::ServiceTable;
use dkrt_lifecycle::ProviderHost;
use dkrt_sdk::OK;

/// The calls into the code of packages, through the rows today's services register and the module's own `init` and `fini`.
///
/// - `start` runs `init` of the file and its commit (`Loader::package_start`): a module's `init` is module code and declares nothing; a service's `init` declares what its
///   header table says and its row joins the table of services at the commit. Nothing is public yet. Today's header has no pools, so the declarations are empty and the
///   manifest's pools are too (RC3 packs them).
/// - `activate` (`Loader::package_activate`) runs after the entries are installed: a service publishes its exports and resident query entry, and the package is live
///   (tick, listing client, intro). `deactivate` undoes it before the entries are restored.
/// - `attach` has nothing to call: today's rows learn of a consumer in the commit of that consumer's own `start` (`Service::module_commit`, on every row), not in a call
///   per service. RC3's `attach` takes its place.
/// - `detach` is the row's `remove` for the consumer's owner id: the service frees what it holds for it. The consumer's `stop` repeats it for every row; it is idempotent.
/// - `can_stop` of a module asks every row `teardown_refusal` (a routed effect refuses); a service has no refusal of its own yet.
/// - `stop` is `fini` and the rest of the unload (`Loader::package_stop`); it cannot refuse, so a record that still has a file or I/O step answers busy and the placer's
///   `free` repeats it.
pub struct RtHost<P: Platform, M: Images, R: Registry>(Handle<P, M, R>);

impl<P: Platform, M: Images, R: Registry> RtHost<P, M, R> {
    pub(super) fn new(h: Handle<P, M, R>) -> Self {
        RtHost(h)
    }
}

impl<P: Platform, M: Images, R: Registry> ProviderHost for RtHost<P, M, R> {
    fn start(&mut self, package: PackageId) -> Result<Declarations, Status> {
        unsafe {
            let (s, l) = self.0.get();
            s.entries_of = None;
            l.package_start(package, &mut s.flight, &mut s.entries)?;
            s.entries_of = Some(package);
            Ok(Declarations::new())
        }
    }

    fn activate(&mut self, package: PackageId) -> Result<(), Status> {
        unsafe {
            let (s, l) = self.0.get();
            l.package_activate(package, &mut s.flight)
        }
    }

    fn deactivate(&mut self, package: PackageId) {
        unsafe {
            let (_, l) = self.0.get();
            l.package_deactivate(package);
        }
    }

    fn attach(&mut self, _service: PackageId, _consumer: PackageId) -> Result<(), Status> {
        Ok(())
    }

    fn detach(&mut self, service: PackageId, consumer: PackageId) {
        // an owner id names a module record: only a module is a consumer of a row
        if consumer.0 as usize >= dkrt_abi::MAX_MODULES {
            return;
        }
        // the table first: the rows are read after it, which is the order they have when a row joins
        let table = services();
        if let Some(rows) = crate::svc_rows::row_of(service.0 as usize) {
            unsafe { ServiceTable { rows, ..table }.remove(consumer.0 + 1) }
        }
    }

    fn can_stop(&mut self, package: PackageId) -> Result<(), Status> {
        if package.0 as usize >= dkrt_abi::MAX_MODULES {
            return Ok(());
        }
        match unsafe { services().teardown_refusal(package.0 + 1) } {
            OK => Ok(()),
            status => Err(status),
        }
    }

    fn stop(&mut self, package: PackageId) {
        unsafe {
            let (s, l) = self.0.get();
            s.stop_busy[package.0 as usize] = l.package_stop(package) != OK;
        }
    }
}
