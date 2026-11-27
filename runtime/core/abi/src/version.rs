//! The version rules of services: a service meets a binding or query when it has the same major version and is not older than the minimum ([`version_satisfies`]); the
//! needs of packages have their own, older rules ([`provider_meets`], [`core_meets`]).
//!
//! Versions are packed `major << 16 | minor << 8 | patch` (`dkneeds::keys::Need::min_version`, a provider's PROVIDES item). The loader applies the rule to
//! `[[service_import]]` bindings and resident queries; the package graph applies [`provider_meets`] and [`core_meets`] to the needs. This is the one copy.

/// True when a provider of `provided` meets a need of at least `minimum`: `provided >= minimum` and the same major version.
#[inline(always)]
pub const fn version_satisfies(provided: u32, minimum: u32) -> bool {
    provided >= minimum && provided >> 16 == minimum >> 16
}

/// True when a service of a package at `provided` meets a need of at least `minimum`: not older, and a minimum of 0 is any version. Where the consumer imports the
/// service's own functions (`major_checked`, a `SERVICE_IMPORTS` item names the service) a minimum from another major version is not met; any other need (the legacy
/// SDK needs carry a runtime version) is met across majors.
#[inline(always)]
pub const fn provider_meets(provided: u32, minimum: u32, major_checked: bool) -> bool {
    provided >= minimum && (!major_checked || minimum == 0 || provided >> 16 == minimum >> 16)
}

/// True when an API of the core at `provided` meets a need of at least `minimum`: a need names a runtime version, so only a newer minimum refuses.
#[inline(always)]
pub const fn core_meets(provided: u32, minimum: u32) -> bool {
    minimum <= provided
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_major_and_not_older_satisfies() {
        assert!(version_satisfies(0x010200, 0x010200));
        assert!(version_satisfies(0x010300, 0x010200));
        assert!(!version_satisfies(0x010100, 0x010200));
        assert!(!version_satisfies(0x020000, 0x010200));
        assert!(!version_satisfies(0x000900, 0x010000));
    }

    #[test]
    fn a_need_of_zero_is_any_version_and_only_a_checked_need_looks_at_the_major() {
        // checked (the consumer imports the service's functions): same major, not older
        assert!(provider_meets(0x010200, 0x010100, true));
        assert!(!provider_meets(0x020000, 0x010100, true));
        assert!(!provider_meets(0x010000, 0x010100, true));
        // a need of 0 is met by any version
        assert!(provider_meets(0x010200, 0, true));
        assert!(provider_meets(0x010200, 0, false));
        // unchecked (a legacy need on a runtime version): a provider of another major is not refused, an older one is
        assert!(provider_meets(0x010000, 0x000100, false));
        assert!(!provider_meets(0x000100, 0x010000, false));
    }

    #[test]
    fn only_a_newer_minimum_than_the_core_refuses() {
        assert!(core_meets(0x000900, 0));
        assert!(core_meets(0x000900, 0x000700));
        assert!(core_meets(0x000900, 0x000900));
        assert!(!core_meets(0x000900, 0x000901));
        assert!(
            core_meets(0x010000, 0x000900),
            "a core of another major still meets an older floor"
        );
    }
}
