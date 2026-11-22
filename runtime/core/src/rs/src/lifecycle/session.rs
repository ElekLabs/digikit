use super::{RtColdFire, RtHost, RtInstallers, RtLinker, RtPlacer, RtSharc};
use crate::loader::discover::PATH_CAP;
use crate::loader::package::{Entries, Flight};
use crate::loader::{Images, Loader, Registry, LOG_NAME};
use crate::admission::Admission;
use core::ptr::addr_of_mut;
use dkrt_abi::package::PackageId;
use dkrt_abi::plan::{Reason, Verdict};
use dkrt_abi::platform::Platform;
use dkrt_abi::provider::Status;
use dkrt_abi::REC_MAX;
use dkrt_lifecycle::{Installers, LiveSet, PackageGraph};
use dkrt_sdk::{DkrtCap, NEEDS_LEN, SEL_MAX};

/// What a package that was refused left for the report of the boot or the live job: the name of the missing module, the log's items and the header's name, as the
/// loader held them when the step failed (the next package overwrites them).
#[derive(Clone, Copy)]
pub struct Outcome {
    pub missing: [u8; NEEDS_LEN],
    /// The log words of a refused provider: the item that the declarations and the table do not both hold (its tag, its first word), and the name hash that the port file did not list.
    pub decl_diff: (u32, u32),
    pub import_hash: u32,
    pub name: [u8; LOG_NAME],
}

impl Outcome {
    pub const NONE: Outcome = Outcome { missing: [0; NEEDS_LEN], decl_diff: (0, 0), import_hash: 0, name: [0; LOG_NAME] };
}

/// The session of one package manager, what its seams share: the loader they drive, the file of the package that is being brought up (the staged bytes stay valid from its place to
/// its activation, because the manager brings packages up one at a time), the entries its start derived, and the paths of the selected modules.
///
/// The seams hold a pointer to this value: it must not move and must outlive the manager. The loader is a static of the UI task on the device and a local of the test.
pub struct Session<P: Platform, M: Images, R: Registry> {
    pub(super) loader: *mut Loader<P, M, R>,
    pub(super) flight: Flight,
    pub(super) entries: Entries,
    /// The package `entries` are of.
    pub(super) entries_of: Option<PackageId>,
    /// The store path of the module at each selection index (NUL terminated, valid while the seams live; null: none).
    pub(super) modules: [*const u8; dkrt_abi::MAX_MODULES],
    /// The packages whose `stop` answered `E_BUSY`: `free` asks again.
    pub(super) stop_busy: [bool; REC_MAX],
    /// The packages whose memory `free` could not release (a file or I/O step of the record still runs).
    pub(super) retained: [bool; REC_MAX],
    /// The packages that are active and whose DSP part `start_dsp` has not started yet.
    pub(crate) dsp_wanted: [bool; REC_MAX],
    /// The rules of the core over the packages admitted so far (`Manifest::admit`): the boot's, or a live load's seeded from the live set.
    pub(super) admission: Admission,
    /// Why admission refused each package (`None`: it did not), for the verdict the boot record shows.
    pub(super) reasons: [Option<Reason>; REC_MAX],
    /// Whether admission admitted each package in this boot or live load.
    pub(super) admitted: [bool; REC_MAX],
    /// The boot's selection count, and whether the services were told which modules stay (once per boot, when every service has been tried: `RtHost::services_up`).
    pub(super) boot_count: usize,
    pub(super) numbered: bool,
    /// What each record's last failed step left.
    pub(super) outcome: [Outcome; REC_MAX],
}

/// A pointer to a [`Session`], held by each seam.
pub(super) struct Handle<P: Platform, M: Images, R: Registry>(pub(super) *mut Session<P, M, R>);

impl<P: Platform, M: Images, R: Registry> Clone for Handle<P, M, R> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<P: Platform, M: Images, R: Registry> Copy for Handle<P, M, R> {}

impl<P: Platform, M: Images, R: Registry> Handle<P, M, R> {
    /// The shared state and the loader. The seams run on one task, one call at a time.
    pub(super) unsafe fn get<'a>(self) -> (&'a mut Session<P, M, R>, &'a mut Loader<P, M, R>) {
        let s = &mut *self.0;
        let l = &mut *s.loader;
        (s, l)
    }
}

impl<P: Platform, M: Images, R: Registry> Session<P, M, R> {
    /// The state for `loader`, with the store paths of the selected modules by selection index (`None`: no module there). By value, for host tests: the session is 4.4 KB, so the
    /// device builds it in place (`Session::init`).
    #[cfg(test)]
    pub fn new(loader: *mut Loader<P, M, R>, modules: [Option<*const u8>; dkrt_abi::MAX_MODULES]) -> Self {
        let mut at = core::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            Self::init(at.as_mut_ptr(), loader);
            let mut s = at.assume_init();
            for (k, m) in modules.iter().enumerate() {
                s.modules[k] = m.unwrap_or(core::ptr::null());
            }
            s
        }
    }

    /// The state for `loader` at `out`, with no module path yet, written field by field from constants: no copy of it is on the stack.
    ///
    /// # Safety
    /// `out` is valid for writes of a session and stays where it is while the seams use it.
    pub unsafe fn init(out: *mut Self, loader: *mut Loader<P, M, R>) {
        const FLIGHT: Flight = Flight::new();
        const ENTRIES: Entries = Entries::new();
        const REASONS: [Option<Reason>; REC_MAX] = [None; REC_MAX];
        const OUTCOMES: [Outcome; REC_MAX] = [Outcome::NONE; REC_MAX];
        addr_of_mut!((*out).loader).write(loader);
        addr_of_mut!((*out).flight).write(FLIGHT);
        addr_of_mut!((*out).entries).write(ENTRIES);
        addr_of_mut!((*out).entries_of).write(None);
        addr_of_mut!((*out).modules).write([core::ptr::null(); dkrt_abi::MAX_MODULES]);
        addr_of_mut!((*out).stop_busy).write([false; REC_MAX]);
        addr_of_mut!((*out).retained).write([false; REC_MAX]);
        addr_of_mut!((*out).dsp_wanted).write([false; REC_MAX]);
        addr_of_mut!((*out).admission).write(Admission::new(&[]));
        addr_of_mut!((*out).reasons).write(REASONS);
        addr_of_mut!((*out).admitted).write([false; REC_MAX]);
        addr_of_mut!((*out).boot_count).write(0);
        addr_of_mut!((*out).numbered).write(true);
        addr_of_mut!((*out).outcome).write(OUTCOMES);
    }

    /// The placer, linker, host and installers of one manager (the manifest is built beside them, `RtManifest::of`).
    pub fn seams(&mut self) -> (RtPlacer<P, M, R>, RtLinker<P, M, R>, RtInstallers<P, M, R>, RtHost<P, M, R>) {
        let h = Handle(self as *mut Self);
        (RtPlacer::new(h), RtLinker::new(h), Installers::new(RtColdFire::new(h), RtSharc::new(h)), RtHost::new(h))
    }

    /// Keep what the loader holds about the step of `package` that just failed, for its report, and pass the result on.
    pub(super) unsafe fn track<T>(&mut self, package: PackageId, r: Result<T, i32>) -> Result<T, i32> {
        if r.is_err() {
            let d = (*self.loader).failure_detail();
            self.outcome[package.0 as usize] = Outcome { missing: d.missing, decl_diff: d.decl_diff, import_hash: d.import_hash, name: d.name };
        }
        r
    }

    /// The start of a boot of `count` modules: the admission starts from `caps` (`loader::boot_caps`) with nothing admitted, and no service has been told which modules stay.
    pub(crate) fn begin_boot(&mut self, caps: &[DkrtCap], count: usize) {
        self.admission = Admission::new(caps);
        self.reasons = [None; REC_MAX];
        self.admitted = [false; REC_MAX];
        self.boot_count = count;
        self.numbered = false;
    }

    /// Why admission refused `package` (`None`: it did not).
    pub(crate) fn reason(&self, package: PackageId) -> Option<Reason> {
        self.reasons[package.0 as usize]
    }

    /// The rules of the core about `package` (`Manifest::admit`). A refusal keeps its reason for the boot record and answers with its load status.
    pub(super) unsafe fn admit(&mut self, package: PackageId) -> Result<(), Status> {
        let rec = package.0 as usize;
        match (*self.loader).admit_package(&mut self.admission, package) {
            Verdict::Accepted => {
                self.admitted[rec] = true;
                Ok(())
            }
            Verdict::Refused(why) => {
                self.reasons[rec] = Some(why);
                Err(why.code())
            }
        }
    }

    /// Tell the services which modules of the boot stay, once, when every service has been tried: a module stays when admission admitted it and every service it needs is up.
    /// A refused module takes no number from a service.
    pub(super) unsafe fn number_keys(&mut self, graph: &PackageGraph, live: &LiveSet) {
        if self.numbered {
            return;
        }
        self.numbered = true;
        let mut stays = [false; SEL_MAX];
        let mut k = 0;
        while k < self.boot_count.min(dkrt_abi::MAX_MODULES) {
            let id = PackageId(k as u8);
            stays[k] = self.admitted[k] && graph.services_of(id).all(|s| live.contains(s));
            k += 1;
        }
        (*self.loader).number_boot_keys(&stays, self.boot_count);
    }

    /// The memory of `package` after its stop, as the manager's take-down frees it: kept when the stop answered busy (it ran the DSP's invalidation, which must not be asked again in the
    /// same pass: the live job's retry, `retry_free`, runs the stop once more), else released.
    pub(super) unsafe fn free_after_stop(&mut self, package: PackageId) {
        let rec = package.0 as usize;
        self.retained[rec] = self.stop_busy[rec] || !(*self.loader).package_free(package);
    }

    /// Run `free` of `package` again: a `stop` that answered busy and a record that could not be released are tried once more. Used by the live job's retry.
    pub(super) unsafe fn retry_free(&mut self, package: PackageId) {
        let l = &mut *self.loader;
        let rec = package.0 as usize;
        if self.stop_busy[rec] {
            self.stop_busy[rec] = l.package_stop(package) != dkrt_sdk::OK;
        }
        self.retained[rec] = self.stop_busy[rec] || !l.package_free(package);
    }

    /// Whether the memory of `package` is still held after its `free`.
    pub fn retained(&self, package: PackageId) -> bool {
        self.retained[package.0 as usize]
    }

    /// The path of `package` into `buf` (NUL terminated) and a pointer to it: the module's selection path, or the provider the search found.
    pub(super) unsafe fn path_of(&self, package: PackageId, buf: &mut [u8; PATH_CAP + 1]) -> Option<*const u8> {
        let rec = package.0 as usize;
        if rec < dkrt_abi::MAX_MODULES {
            let p = self.modules[rec];
            return if p.is_null() { None } else { Some(p) };
        }
        if (*self.loader).provider_path(package, buf) {
            Some(buf.as_ptr())
        } else {
            None
        }
    }
}
