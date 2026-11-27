/// Where a package is in its life (plan section 8.1). A package reaches `Active` one state at a time; a failure unwinds the states it reached. The take-down
/// states are not kept except the two that can wait: `Detaching`, a package whose services still give back what they hold for it (a payload retire answered
/// pending), and `RestorePending`, a package that is detached and has released its claims but whose entries are still in firmware because the installer
/// answered "busy". The order of the variants is the order of a life: a take-down compares states.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum State {
    /// Memory is given.
    Placed,
    /// Imports are bound to live services.
    Linked,
    /// The package's object exists (a service declared its resources, a module ran its init).
    Started,
    /// Its claims are in the ledger.
    Claimed,
    /// Its entries are in firmware.
    Installed,
    /// Public: its exports and resident entry points are visible, its tick runs. Nothing is public before the entries are installed.
    Activated,
    /// The services it needs are attaching it over more than one UI pass (a payload install answered pending): [`PackageManager::step`](crate::PackageManager::step)
    /// calls them again. The package is live but not up: its failure takes its closure down.
    Attaching,
    /// Attached to every service it needs: up.
    Active,
    /// Its detach answered pending: the take-down waits for a retry, which asks the services again before it goes on.
    Detaching,
    /// Taken down as far as the restore of its entries: waiting for a retry.
    RestorePending,
}
