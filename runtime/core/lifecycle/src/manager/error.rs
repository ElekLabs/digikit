use crate::install::InstallError;
use crate::refusal::Refusal;
use crate::state::State;
use crate::PackageId;
use dkrt_abi::provider::Status;
use dkrt_fit::ledger::{LedgerError, PoolShort};

/// Why [`PackageManager`](super::PackageManager) refused an operation or stopped one. Each variant names the package.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleError {
    /// The graph refuses (unmet or incompatible need, unknown package, bad table, cycle).
    Graph(Refusal),
    /// The core's own pools could not be declared.
    CoreResource(LedgerError),
    /// The claims of the package do not fit the ledger (the pools of the packages before it in the order counted); it was not placed. A claim on a pool nobody declared is `free: 0`.
    Short {
        package: PackageId,
        short: PoolShort,
    },
    /// A ledger refusal other than a shortage (a pool declared twice, a full table).
    Ledger {
        package: PackageId,
        error: LedgerError,
    },
    /// The service's `start` declared other resources than its header.
    Declaration { package: PackageId },
    /// A seam refused while the package was entering `entering`; the package was unwound.
    Failed {
        package: PackageId,
        entering: State,
        status: Status,
    },
    /// The installer refused the entries; the package was unwound.
    Install {
        package: PackageId,
        error: InstallError,
    },
    /// A service the package needs was refused, so the package was not tried.
    NeedRefused {
        package: PackageId,
        service: PackageId,
    },
    /// A service said it cannot stop now; nothing was touched.
    CannotStop { package: PackageId, status: Status },
    /// The restore of the entries answered `E_BUSY`: the package stays in `RestorePending`; finish with `retry_pending`.
    Busy { package: PackageId },
    /// The restore failed for another reason; the package stays in `RestorePending`; finish with `retry_pending`.
    RestoreFailed {
        package: PackageId,
        error: InstallError,
    },
    /// A take-down waits for `retry_pending`: no other operation may run.
    RestorePending { package: PackageId },
    /// `retry_pending` was called and no take-down waits.
    NothingPending,
    /// The package is not up.
    NotLive { package: PackageId },
    /// The package is up already.
    AlreadyLive { package: PackageId },
}
