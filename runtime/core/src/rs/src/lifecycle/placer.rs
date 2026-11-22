use super::session::Handle;
use crate::loader::discover::PATH_CAP;
use crate::loader::{Images, Registry};
use dkrt_abi::package::PackageId;
use dkrt_abi::plan::Verdict;
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
            // a service the plan refused is not placed: its refusal is its result
            if let Some(j) = (package.0 as usize).checked_sub(dkrt_abi::MAX_MODULES) {
                if s.provider_refusal.get(j).copied().unwrap_or(0) != 0 {
                    let r = Err(s.provider_refusal[j]);
                    return s.track(package, r);
                }
            }
            if (package.0 as usize) < dkrt_abi::MAX_MODULES {
                // every service of the boot has been tried by now: the services number the keys of the modules that stay (the checks of the module's own sections read the
                // numbers), and a module whose service did not come up is refused before anything of it is placed
                s.settle_once();
                // the plan's verdicts are the boot's (`plan_count` is 0 again once the boot has reported): a live load is decided by the manager from the services then
                if let Verdict::Refused(why) = s.plan.verdicts[package.0 as usize] {
                    if s.plan_count != 0 {
                        return s.track(package, Err(why.code()));
                    }
                }
            }
            let mut buf = [0u8; PATH_CAP + 1];
            let path = s.path_of(package, &mut buf).ok_or(E_NOT_FOUND)?;
            let r = l.package_place(package, path, &mut s.flight);
            s.track(package, r)
        }
    }

    fn free(&mut self, package: PackageId) {
        unsafe {
            let (s, _) = self.0.get();
            // a `stop` that answered busy runs again; once the record is down, there is nothing left to release
            s.retry_free(package);
        }
    }
}
