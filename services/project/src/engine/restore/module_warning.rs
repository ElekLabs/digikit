//! Why a module of the list is not loaded.

/// Why a module of the list is not loaded: the warning of `Env::warn_module`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModuleWarning {
    /// The module's file is not on the card.
    Missing,
    /// The runtime refused the load because the module does not fit (memory, pool, plan or DSP space).
    NoFit,
    /// The runtime refused or failed the load for another reason (not compatible, read error, ...), or the store could not be searched.
    Refused,
    /// The module is loaded but not saved in the selection: it is not loaded again at the next boot.
    NotKept,
}
