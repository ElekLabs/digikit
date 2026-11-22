use super::{RtColdFire, RtHost, RtInstallers, RtLinker, RtPlacer, RtSharc};
use crate::loader::discover::PATH_CAP;
use crate::loader::package::{Entries, Flight};
use crate::loader::{Images, Loader, Registry, LOG_NAME};
use crate::plan::Plan;
use core::ptr::addr_of_mut;
use dkrt_abi::package::PackageId;
use dkrt_abi::plan::Verdict;
use dkrt_abi::platform::Platform;
use dkrt_abi::REC_MAX;
use dkrt_lifecycle::Installers;
use dkrt_sdk::{NEEDS_LEN, PROVIDER_MAX, SEL_MAX};

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
    /// The plan of the boot in progress: the verdicts the boot reports, the providers the plan refused, and whether `settle` ran (it runs once, before the first module starts, so
    /// the services number the keys of the modules that stay).
    pub(super) plan: Plan,
    pub(super) plan_count: usize,
    pub(super) settled: bool,
    /// The load status of a provider the plan refused (0: none): its place answers it.
    pub(super) provider_refusal: [i32; PROVIDER_MAX],
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
        const PLAN: Plan = Plan {
            n: 0,
            verdicts: [Verdict::Accepted; SEL_MAX],
            order: [0; SEL_MAX],
            dsp_load: (0, 0),
            np: 0,
            provider_verdicts: [Verdict::Accepted; PROVIDER_MAX],
            provider_order: [0; PROVIDER_MAX],
        };
        const OUTCOMES: [Outcome; REC_MAX] = [Outcome::NONE; REC_MAX];
        addr_of_mut!((*out).loader).write(loader);
        addr_of_mut!((*out).flight).write(FLIGHT);
        addr_of_mut!((*out).entries).write(ENTRIES);
        addr_of_mut!((*out).entries_of).write(None);
        addr_of_mut!((*out).modules).write([core::ptr::null(); dkrt_abi::MAX_MODULES]);
        addr_of_mut!((*out).stop_busy).write([false; REC_MAX]);
        addr_of_mut!((*out).retained).write([false; REC_MAX]);
        addr_of_mut!((*out).dsp_wanted).write([false; REC_MAX]);
        addr_of_mut!((*out).plan).write(PLAN);
        addr_of_mut!((*out).plan_count).write(0);
        addr_of_mut!((*out).settled).write(true);
        addr_of_mut!((*out).provider_refusal).write([0; PROVIDER_MAX]);
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

    /// Run the plan's settle once: a module the plan accepted whose hard need only a failed service could meet is refused, and the services number the keys of the modules that
    /// stay. Needs every service of the boot attempted, so it runs at the first module's start (the manager brings the services up first) or at the end of the boot.
    pub(super) unsafe fn settle_once(&mut self) {
        if !self.settled {
            self.settled = true;
            (*self.loader).settle(&mut self.plan, self.plan_count);
        }
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
