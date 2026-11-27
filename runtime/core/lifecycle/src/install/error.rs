use dkrt_abi::provider::Status;

/// Why [`Installers`](super::Installers) refused or failed. A runtime type of this crate, not an SDK status code: the mapping to SDK status codes
/// happens at the ABI boundary (RC4), not here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallError {
    /// More entries than `MAX_ENTRIES`; nothing was touched.
    TableFull,
    /// The owner id is past `MAX_PACKAGES`; nothing was touched.
    OwnerOutOfRange,
    /// The owner is installed, or partly installed; restore it first.
    AlreadyInstalled,
    /// An install failed and the rollback of the backend that had succeeded failed too: the owner is partly installed. The caller calls `restore`
    /// again until it answers `Ok`.
    Partial,
    /// A backend answered with this status (for `restore`, the first error of the backends asked; `E_BUSY` means "retry next pass").
    Backend(Status),
}
