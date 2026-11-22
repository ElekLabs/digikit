use super::session::Handle;
use crate::loader::discover::PATH_CAP;
use crate::loader::{Images, Registry};
use dkrt_abi::package::PackageId;
use dkrt_abi::platform::Platform;
use dkrt_abi::provider::Status;
use dkrt_lifecycle::Placer;
use dkrt_sdk::E_NOT_FOUND;

/// Gives a package its memory: the checks of its file, the arena block, the image with the relocations that name the image, and its record
/// (`Loader::package_place`; `Loader::package_free` for a record that was placed and not started).
pub struct RtPlacer<P: Platform, M: Images, R: Registry>(Handle<P, M, R>);

impl<P: Platform, M: Images, R: Registry> RtPlacer<P, M, R> {
    pub(super) fn new(h: Handle<P, M, R>) -> Self {
        RtPlacer(h)
    }
}

impl<P: Platform, M: Images, R: Registry> Placer for RtPlacer<P, M, R> {
    fn place(&mut self, package: PackageId) -> Result<u32, Status> {
        unsafe {
            let (s, l) = self.0.get();
            let mut buf = [0u8; PATH_CAP + 1];
            let path = s.path_of(package, &mut buf).ok_or(E_NOT_FOUND)?;
            l.package_place(package, path, &mut s.flight)
        }
    }

    fn free(&mut self, package: PackageId) {
        unsafe {
            let (s, l) = self.0.get();
            let rec = package.0 as usize;
            // a `stop` that answered busy runs again; once the record is down, there is nothing left to release
            if s.stop_busy[rec] {
                s.stop_busy[rec] = l.package_stop(package) != dkrt_sdk::OK;
            }
            s.retained[rec] = s.stop_busy[rec] || !l.package_free(package);
        }
    }
}
