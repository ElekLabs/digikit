use crate::{Entry, InstallError, PackageId, PackageInstaller, Step, Trace, Traced};

/// The status an install error is logged with: a backend's own status, and for the errors of the installer itself a code of this crate (1001 and up) that is no SDK status.
fn status_of(r: &Result<(), InstallError>) -> i32 {
    // comparisons, not a `match`: a dense match compiles to a jump table, which the runtime's ColdFire code check reads as illegal code
    let Err(e) = r else {
        return 0;
    };
    if let InstallError::Backend(s) = e {
        *s
    } else if *e == InstallError::TableFull {
        1001
    } else if *e == InstallError::OwnerOutOfRange {
        1002
    } else if *e == InstallError::AlreadyInstalled {
        1003
    } else {
        1004
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
