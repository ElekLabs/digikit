//! The version rule of services: a provider meets a need when it has the same major version and is not older than the minimum.
//!
//! Versions are packed `major << 16 | minor << 8 | patch` (`dkneeds::keys::Need::min_version`, a provider's PROVIDES item). The loader applies the rule to
//! `[[service_import]]` bindings and resident queries; the package graph applies it to every need. This is the one copy.

/// True when a provider of `provided` meets a need of at least `minimum`: `provided >= minimum` and the same major version.
#[inline(always)]
pub const fn version_satisfies(provided: u32, minimum: u32) -> bool {
    provided >= minimum && provided >> 16 == minimum >> 16
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
}
