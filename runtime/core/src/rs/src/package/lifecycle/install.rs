use super::session::Handle;
use crate::package::loader::{Images, Registry};
use dkrt_abi::package::PackageId;
use dkrt_abi::platform::Platform;
use dkrt_abi::provider::Status;
use dkrt_lifecycle::{Cpu, Entry, Installer, Installers};
use dkrt_sdk::E_BUSY;

/// The ColdFire backend of the entries: the loader's hook installer (`loader/install.rs`) behind the `Installer` interface (`Loader::package_install`). A hook entry is a
/// `jmp.l` over the site (the length and the check value of the stock bytes are the port file's, found by the site's address); a slot entry fills a pointer word of the
/// image. Targets are absolute addresses. Entries other than these two are refused. The real per-CPU backends come in RC4.
pub struct RtColdFire<P: Platform, M: Images, R: Registry>(Handle<P, M, R>);

impl<P: Platform, M: Images, R: Registry> RtColdFire<P, M, R> {
    pub(super) fn new(h: Handle<P, M, R>) -> Self {
        RtColdFire(h)
    }
}

impl<P: Platform, M: Images, R: Registry> Installer for RtColdFire<P, M, R> {
    fn cpu(&self) -> Cpu {
        Cpu::ColdFire
    }

    fn install(&mut self, owner: PackageId, entries: &[Entry]) -> Result<(), Status> {
        unsafe {
            let (_, l) = self.0.get();
            l.package_install(owner, entries)
        }
    }

    fn restore(&mut self, owner: PackageId) -> Result<(), Status> {
        unsafe {
            let (_, l) = self.0.get();
            l.package_restore(owner);
        }
        Ok(())
    }
}

/// The SHARC backend: the slots of a service file's DSP code in the slot block of `core/dsp-link` (`DspInstaller`), behind the `Installer` interface. `install` claims the
/// slots (the stubs stay stock until the DSP acknowledges the code: `dsp_step` writes them); `restore` sends the clear sequence and answers `E_BUSY` when the DSP does not take it
/// (the entries stay and the manager retries). Entries other than DSP slots are refused.
pub struct RtSharc<P: Platform, M: Images, R: Registry>(Handle<P, M, R>);

impl<P: Platform, M: Images, R: Registry> RtSharc<P, M, R> {
    pub(super) fn new(h: Handle<P, M, R>) -> Self {
        RtSharc(h)
    }
}

impl<P: Platform, M: Images, R: Registry> Installer for RtSharc<P, M, R> {
    fn cpu(&self) -> Cpu {
        Cpu::Sharc
    }

    fn install(&mut self, owner: PackageId, entries: &[Entry]) -> Result<(), Status> {
        unsafe {
            let (_, l) = self.0.get();
            l.package_install_dsp(owner, entries)
        }
    }

    fn restore(&mut self, owner: PackageId) -> Result<(), Status> {
        unsafe {
            let (_, l) = self.0.get();
            if l.package_restore_dsp(owner) {
                Ok(())
            } else {
                Err(E_BUSY)
            }
        }
    }
}

/// The installers of the runtime: ColdFire entries through the loader's installer, DSP slots through its DSP backend.
pub type RtInstallers<P, M, R> = Installers<RtColdFire<P, M, R>, RtSharc<P, M, R>>;
