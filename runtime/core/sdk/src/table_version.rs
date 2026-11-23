//! The compatibility rule of a resident table version, defined once: the system package header and the resident table check both call it.
//! The majors are equal and the package's minor is not above the core's (the core's table only grows by appending, so a package that wants fewer
//! functions runs on a newer core; one that wants more does not).

/// Why a package's table version does not fit the core's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VersionMismatch {
    /// The majors differ.
    Major,
    /// The majors are equal and the package wants a newer minor than the core has.
    Minor,
}

/// Can a core with table `core` = (major, minor) run a package built against `package` = (major, minor)?
pub fn check(package: (u16, u16), core: (u16, u16)) -> Result<(), VersionMismatch> {
    if package.0 != core.0 {
        Err(VersionMismatch::Major)
    } else if package.1 > core.1 {
        Err(VersionMismatch::Minor)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
