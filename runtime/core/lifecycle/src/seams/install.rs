use crate::install::{Entry, InstallError, Installer, Installers};
use crate::PackageId;

/// Installs the entries of a package into firmware and takes them back: the one installer the manager sees ([`Installers`] in the runtime).
pub trait PackageInstaller {
    /// Install all `entries` of `owner`, or none.
    fn install(&mut self, owner: PackageId, entries: &[Entry]) -> Result<(), InstallError>;

    /// Return the firmware to stock for `owner`; idempotent. `Busy` means "not now": the entries stay and the caller retries.
    fn restore(&mut self, owner: PackageId) -> Result<(), InstallError>;
}

impl<CF: Installer, DSP: Installer> PackageInstaller for Installers<CF, DSP> {
    fn install(&mut self, owner: PackageId, entries: &[Entry]) -> Result<(), InstallError> {
        Installers::install(self, owner, entries)
    }

    fn restore(&mut self, owner: PackageId) -> Result<(), InstallError> {
        Installers::restore(self, owner)
    }
}
