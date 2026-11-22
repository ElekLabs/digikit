use super::session::Handle;
use crate::loader::{Images, Registry};
use dkrt_abi::package::PackageId;
use dkrt_abi::platform::Platform;
use dkrt_abi::provider::Status;
use dkrt_lifecycle::{Cpu, Entry, Installer, Installers};
use dkrt_sdk::E_CONTEXT;

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

/// The SHARC backend until the DSP part comes up through the manager (RC2b-2): a package with a DSP section is refused at its place, so no entry of this CPU reaches it.
pub struct NoSharc;

impl Installer for NoSharc {
    fn cpu(&self) -> Cpu {
        Cpu::Sharc
    }

    fn install(&mut self, _owner: PackageId, _entries: &[Entry]) -> Result<(), Status> {
        Err(E_CONTEXT)
    }

    fn restore(&mut self, _owner: PackageId) -> Result<(), Status> {
        Ok(())
    }
}

/// The installers of the runtime: ColdFire entries through the loader's installer, no SHARC entries.
pub type RtInstallers<P, M, R> = Installers<RtColdFire<P, M, R>, NoSharc>;
