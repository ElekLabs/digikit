use super::carried_state::Donor;
use super::service_copy::ServiceCopy;
use super::session::Handle;
use crate::package::handlers::services;
use crate::package::loader::{Images, Registry};
use dkrt_abi::package::PackageId;
use dkrt_abi::package_platform::PackagePlatform;
use dkrt_abi::provider::{Declarations, Status};
use dkrt_lifecycle::{LiveSet, PackageGraph, ProviderHost};
use dkrt_sdk::OK;

/// The calls into the code of packages, through the rows today's services register and the module's own `init` and `fini`.
///
/// - `start` runs `init` of the file and its commit (`Loader::package_start`): a module's `init` is module code and declares nothing; a service's `init` declares what its
///   header table says and its row joins the table of services at the commit. Nothing is public yet. Today's header has no pools, so the declarations are empty and the
///   manifest's pools are too (RC3 packs them).
/// - `activate` (`Loader::package_activate`) runs after the entries are installed: a service publishes its exports and resident query entry, and the package is live
///   (tick, listing client, intro). `deactivate` undoes it before the entries are restored.
/// - `attach` applies the records of the consumer module, once, over the rows of the services it needs (`Loader::package_attach`): every row's `attach` in the order of the table,
///   then each record in file order to the row that owns its kind (`Records::apply`), so rows the services share are numbered in the order of the file. The
///   module's `start` only keeps its section. A module's needs cover the services that own its record kinds (patchkit packs a hard need for each loaded owner of a kind of its
///   `KEYS`). The services' `module_commit` (the records' check, measure and write) still runs for every row in `start`.
/// - `start_dsp` hands the DSP part of an active package to the DSP loader (`Loader::package_start_dsp`); it comes over the UI passes (`Rt::dsp_step`).
/// - `detach` is `Provider::detach` of the same rows for the consumer's owner id, the last row first: the services free what they hold for it. The consumer's `stop` repeats it for
///   every row; it is idempotent.
/// - `can_stop` of a module asks every row `Provider::can_stop` (a routed effect refuses): the manager asks every package it would take down. `can_stop` names no
///   consumer. `can_stop` of a service asks its own row only.
/// - `stop` is `fini` and the rest of the unload (`Loader::package_stop`); it cannot refuse, so a record that still has a file or I/O step answers busy and the placer's
///   `free` repeats it.
pub struct RtHost<P: PackagePlatform, M: Images, R: Registry>(Handle<P, M, R>);

impl<P: PackagePlatform, M: Images, R: Registry> RtHost<P, M, R> {
    pub(super) fn new(h: Handle<P, M, R>) -> Self {
        RtHost(h)
    }
}

impl<P: PackagePlatform, M: Images, R: Registry> ProviderHost for RtHost<P, M, R> {
    fn start(&mut self, package: PackageId) -> Result<Declarations, Status> {
        unsafe {
            let (s, l) = self.0.get();
            s.entries_of = None;
            let r = l.package_start(package, &mut s.flight, &mut s.entries);
            s.track(package, r)?;
            s.entries_of = Some(package);
            Ok(Declarations::new())
        }
    }

    fn activate(&mut self, package: PackageId) -> Result<(), Status> {
        unsafe {
            let (s, l) = self.0.get();
            let r = l.package_activate(package, &mut s.flight);
            let dsp = s.track(package, r)?;
            s.dsp_wanted[package.0 as usize] = dsp;
            Ok(())
        }
    }

    fn deactivate(&mut self, package: PackageId) {
        unsafe {
            let (_, l) = self.0.get();
            l.package_deactivate(package);
        }
    }

    fn services_up(&mut self, graph: &PackageGraph, live: &LiveSet) {
        unsafe {
            let (s, _) = self.0.get();
            s.number_keys(graph, live);
        }
    }

    fn attach_rank(&self, service: PackageId) -> u32 {
        crate::package::provider_rows::rank_of(service.0 as usize)
    }

    fn attach(&mut self, consumer: PackageId, services: &[PackageId]) -> Result<(), Status> {
        unsafe {
            let (s, l) = self.0.get();
            let r = l.package_attach(consumer, services);
            s.track(consumer, r)
        }
    }

    fn detach(&mut self, consumer: PackageId, services: &[PackageId]) {
        unsafe {
            let (_, l) = self.0.get();
            l.package_detach(consumer, services);
        }
    }

    fn can_stop(&mut self, package: PackageId) -> Result<(), Status> {
        if package.0 as usize >= dkrt_abi::MAX_MODULES {
            // a service answers for itself: `E_BUSY` is work in flight, which the live job asks again (a service that adopted no answer says OK)
            return match ServiceCopy::of(package.0 as usize).map_or(OK, |c| unsafe { c.can_stop() }) {
                OK => Ok(()),
                status => Err(status),
            };
        }
        match unsafe { services().can_stop() } {
            OK => Ok(()),
            status => Err(status),
        }
    }

    fn export_state(&mut self, package: PackageId) {
        unsafe {
            let (s, _) = self.0.get();
            if let Some(copy) = ServiceCopy::of(package.0 as usize) {
                s.carried.export(package.0 as usize, &copy);
            }
        }
    }

    fn import_state(&mut self, package: PackageId) -> Result<(), Status> {
        unsafe {
            let (s, _) = self.0.get();
            let Some(copy) = ServiceCopy::of(package.0 as usize) else {
                return Ok(());
            };
            let (status, outcome) = s.carried.import(package.0 as usize, &copy);
            super::trace::emit_state(copy.name(), &outcome);
            if status == OK {
                Ok(())
            } else {
                Err(status)
            }
        }
    }

    fn start_dsp(&mut self, package: PackageId) -> bool {
        unsafe {
            let (s, l) = self.0.get();
            let rec = package.0 as usize;
            if !s.dsp_wanted[rec] {
                return false;
            }
            s.dsp_wanted[rec] = false;
            l.package_start_dsp(package);
            true
        }
    }

    fn stop(&mut self, package: PackageId) {
        unsafe {
            let (s, l) = self.0.get();
            s.stop_busy[package.0 as usize] = l.package_stop(package) != OK;
        }
    }
}
