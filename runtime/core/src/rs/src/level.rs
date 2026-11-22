//! The level a boot runs at: normal, safe or core-only. A pure decision over what the resident core has read (the selection record, the key, the
//! crash records, the boot counter and the state of the system package); it reads nothing and writes nothing. Design: docs/plans/resident-core-and-system-package.md, section 5.

use dkrt_sdk::*;

/// Boots in a row that did not stay up for `STABLE_BLOCKS` after which the next one runs the resident core only (no system package): the crashes
/// went on in safe mode, so the package or the core is the likely cause. Only the runtime reads it.
pub const CORE_ONLY_AFTER: u32 = 4;

/// What the system package turned out to be when stage 1 looked for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageStatus {
    /// Found, hash and version fine.
    Ok,
    /// No package file on the card.
    Missing,
    /// The header asks for a resident table this core does not have.
    Incompatible,
    /// The file is not a whole package: too short, of another size than its header says, a body that does not match the hash, or no room to place it.
    Damaged,
    /// The card could not be read for the package (a read error that stayed, or a store that stayed busy). The card may be fine on the next boot.
    Unreadable,
}

/// What the boot found out before it decides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inputs {
    /// No selection copy could be read.
    pub record_unreadable: bool,
    /// The selection record asked for safe mode once.
    pub safe_once: bool,
    /// The safe-mode key is held at power-on.
    pub safe_key_held: bool,
    /// The boot before left a DSP hang record.
    pub dsp_hang_seen: bool,
    /// The boot before left a panic record.
    pub panic_seen: bool,
    /// Boots counted in the record that did not stay up for `STABLE_BLOCKS`.
    pub boot_counter: u32,
    pub package: PackageStatus,
}

/// Why a boot is safe. The discriminant is the `SAFE_*` value of `DkrtBootInfo.reason` (`code`; no `match`: on a ColdFire a dense `match` becomes a
/// PC-relative jump table, which is not ColdFire code).
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SafeReason {
    Unreadable = SAFE_READ,
    Flag = SAFE_FLAG,
    Key = SAFE_KEY,
    DspHang = SAFE_DSP,
    Panic = SAFE_PANIC,
    Counter = SAFE_COUNTER,
}

impl SafeReason {
    pub fn code(self) -> u32 {
        self as u32
    }
}

/// Why a boot runs the resident core only. `counts_as_crash` tells the boot whether the boot counter may count it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreOnlyReason {
    PackageMissing,
    PackageIncompatible,
    PackageDamaged,
    PackageUnreadable,
    /// The counter reached `CORE_ONLY_AFTER`: crashes went on in safe mode.
    Crashes,
}

impl CoreOnlyReason {
    /// A package that cannot run is a card state, not a crash: it is not counted (design decision 2). Repeated crashes are.
    pub fn counts_as_crash(self) -> bool {
        self == CoreOnlyReason::Crashes
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Normal,
    Safe(SafeReason),
    CoreOnly(CoreOnlyReason),
}

/// Do the boots counted in the record call for safe mode (`SAFE_AFTER` boots that did not reach the stable period)?
pub fn counter_is_safe(boot_counter: u32) -> bool {
    boot_counter >= SAFE_AFTER
}

/// Do they call for the core-only level (`CORE_ONLY_AFTER`)?
pub fn counter_is_core_only(boot_counter: u32) -> bool {
    boot_counter >= CORE_ONLY_AFTER
}

/// Decide the level. Core-only comes first: a package that cannot run leaves no menu for safe mode, and a counter past the second threshold says
/// that safe mode itself keeps crashing. Then the safe triggers, in the order of the boot's old if-chain (the first one found is the reason).
pub fn decide(i: &Inputs) -> Level {
    if i.package == PackageStatus::Missing {
        return Level::CoreOnly(CoreOnlyReason::PackageMissing);
    }
    if i.package == PackageStatus::Incompatible {
        return Level::CoreOnly(CoreOnlyReason::PackageIncompatible);
    }
    if i.package == PackageStatus::Damaged {
        return Level::CoreOnly(CoreOnlyReason::PackageDamaged);
    }
    if i.package == PackageStatus::Unreadable {
        return Level::CoreOnly(CoreOnlyReason::PackageUnreadable);
    }
    // an unreadable record has no counter: what the boot holds then is the default, not a count
    if !i.record_unreadable && counter_is_core_only(i.boot_counter) {
        return Level::CoreOnly(CoreOnlyReason::Crashes);
    }
    if i.record_unreadable {
        Level::Safe(SafeReason::Unreadable)
    } else if i.safe_once {
        Level::Safe(SafeReason::Flag)
    } else if i.safe_key_held {
        Level::Safe(SafeReason::Key)
    } else if i.dsp_hang_seen {
        Level::Safe(SafeReason::DspHang)
    } else if i.panic_seen {
        Level::Safe(SafeReason::Panic)
    } else if counter_is_safe(i.boot_counter) {
        Level::Safe(SafeReason::Counter)
    } else {
        Level::Normal
    }
}

#[cfg(test)]
mod tests;
