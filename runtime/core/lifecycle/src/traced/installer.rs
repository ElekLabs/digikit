use crate::{Entry, InstallError, PackageId, PackageInstaller, Step, Trace, Traced};

use dkrt_sdk::E_BUSY;

/// The status an install error is logged with: a backend's own status, and for the errors of the installer itself a code of this crate (1001 and up) that is no SDK status.
fn status_of(r: &Result<(), InstallError>) -> i32 {
    // comparisons, not a `match`: a dense match compiles to a jump table, which the runtime's ColdFire code check reads as illegal code
    let Err(e) = r else {
        return 0;
    };
    if let InstallError::Backend(s) = e {
        *s
    } else if let InstallError::Busy { .. } = e {
        E_BUSY
    } else if *e == InstallError::TableFull {
        1001
    } else if *e == InstallError::OwnerOutOfRange {
        1002
    } else if *e == InstallError::AlreadyInstalled {
        1003
    } else if *e == InstallError::Partial {
        1004
    } else {
        // a variant `status_of_covers_every_error` does not know yet fails there (its match stops compiling)
        STATUS_UNKNOWN
    }
}

/// The status of an error `status_of` has no code for.
const STATUS_UNKNOWN: i32 = 1000;

#[cfg(test)]
mod tests {
    use super::*;

    /// One code for each error, written as a `match`: a new variant stops this from compiling until it has a code in `status_of`.
    fn expected(e: InstallError) -> i32 {
        match e {
            InstallError::TableFull => 1001,
            InstallError::OwnerOutOfRange => 1002,
            InstallError::AlreadyInstalled => 1003,
            InstallError::Partial => 1004,
            InstallError::Backend(s) => s,
            InstallError::Busy { .. } => E_BUSY,
        }
    }

    #[test]
    fn status_of_covers_every_error() {
        for e in [
            InstallError::TableFull,
            InstallError::OwnerOutOfRange,
            InstallError::AlreadyInstalled,
            InstallError::Partial,
            InstallError::Backend(-7),
            InstallError::Busy { task: Some(1) },
        ] {
            assert_eq!(status_of(&Err(e)), expected(e));
        }
        assert_eq!(status_of(&Ok(())), 0);
    }
}

impl<S: PackageInstaller, T: Trace> PackageInstaller for Traced<S, T> {
    fn install(&mut self, owner: PackageId, entries: &[Entry]) -> Result<(), InstallError> {
        self.run(
            Step::Install,
            owner,
            |s| s.install(owner, entries),
            status_of,
        )
    }

    fn restore(&mut self, owner: PackageId) -> Result<(), InstallError> {
        self.run(Step::Restore, owner, |s| s.restore(owner), status_of)
    }
}
