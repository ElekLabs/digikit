use super::{Cpu, Entry, InstallError};
use crate::PackageId;
use dkrt_abi::provider::Status;

/// Writes and restores the entries of packages for one CPU.
///
/// - `install` is called once per owner with all of that owner's entries, of every CPU; the backend installs only the entries whose CPU is its own
///   (and none, with `Ok`, when there are none). A second `install` for an owner that is installed is refused.
/// - `restore` is idempotent: it returns the firmware to stock for everything `owner` installed (table pointers and bounds written as
///   `Entry::Patch` included), and is a no-op `Ok` for an owner with nothing installed.
pub trait Installer {
    /// The CPU whose entries this backend installs.
    fn cpu(&self) -> Cpu;

    /// Install the entries of `entries` that belong to [`cpu`](Self::cpu) for `owner`, or none: on an error nothing of this call stays installed.
    fn install(&mut self, owner: PackageId, entries: &[Entry]) -> Result<(), Status>;

    /// Undo everything `owner` installed. [`InstallError::Busy`] means "not now, retry next pass"; the entries stay. Any other refusal is a [`InstallError::Backend`] status.
    fn restore(&mut self, owner: PackageId) -> Result<(), InstallError>;
}
