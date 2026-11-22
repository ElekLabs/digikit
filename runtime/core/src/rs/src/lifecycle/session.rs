use super::{NoSharc, RtColdFire, RtHost, RtInstallers, RtLinker, RtPlacer};
use crate::loader::discover::PATH_CAP;
use crate::loader::package::{Entries, Flight};
use crate::loader::{Images, Loader, Registry};
use dkrt_abi::package::PackageId;
use dkrt_abi::platform::Platform;
use dkrt_abi::REC_MAX;
use dkrt_lifecycle::Installers;

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
    /// The state for `loader`, with the store paths of the selected modules by selection index (`None`: no module there).
    pub fn new(loader: *mut Loader<P, M, R>, modules: [Option<*const u8>; dkrt_abi::MAX_MODULES]) -> Self {
        let mut paths = [core::ptr::null(); dkrt_abi::MAX_MODULES];
        for (k, m) in modules.iter().enumerate() {
            paths[k] = m.unwrap_or(core::ptr::null());
        }
        Session {
            loader,
            flight: Flight::new(),
            entries: Entries::new(),
            entries_of: None,
            modules: paths,
            stop_busy: [false; REC_MAX],
            retained: [false; REC_MAX],
        }
    }

    /// The placer, linker, host and installers of one manager (the manifest is built beside them, `RtManifest::of`).
    pub fn seams(&mut self) -> (RtPlacer<P, M, R>, RtLinker<P, M, R>, RtInstallers<P, M, R>, RtHost<P, M, R>) {
        let h = Handle(self as *mut Self);
        (RtPlacer::new(h), RtLinker::new(h), Installers::new(RtColdFire::new(h), NoSharc), RtHost::new(h))
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
