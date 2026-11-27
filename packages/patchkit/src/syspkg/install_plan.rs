//! Transport-independent system package installation order.

use serde::Serialize;
use std::fmt;

/// Steps an installer executes in order. Snake-case names are the transport API.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallAction {
    Put,
    Verify,
    Retry,
    Restart,
    WaitForBoot,
    ConfirmSupport,
}

/// When package installation needs a different entry point or is already healthy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallRefusal {
    AlreadyHealthy,
    RecoveryRequired,
}

impl fmt::Display for InstallRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyHealthy => f.write_str("The target already has module support."),
            Self::RecoveryRequired => {
                f.write_str("The verified system package needs an explicit recovery retry.")
            }
        }
    }
}

impl std::error::Error for InstallRefusal {}

/// Plan installation from checked target facts. Matching bytes require a fresh full-file VERIFY.
///
/// A successful empty plan means the matching package is already active. On a replacement,
/// a previously active package requires evidence of a new boot before MODULES can confirm success.
pub fn install_plan(
    repair: bool,
    module_support: bool,
    verified_package_on_card: bool,
) -> Result<&'static [InstallAction], InstallRefusal> {
    use InstallAction::*;
    match (repair, module_support, verified_package_on_card) {
        (true, true, _) => Err(InstallRefusal::AlreadyHealthy),
        (false, true, true) => Ok(&[]),
        (false, true, false) => Ok(&[Put, Verify, Restart, WaitForBoot, ConfirmSupport]),
        (false, false, true) => Err(InstallRefusal::RecoveryRequired),
        (true, false, true) => Ok(&[Retry, Restart, ConfirmSupport]),
        (_, false, false) => Ok(&[Put, Verify, Restart, ConfirmSupport]),
    }
}

#[cfg(test)]
mod tests {
    use super::{InstallAction::*, InstallRefusal::*, install_plan};

    #[test]
    fn healthy_recovery_is_refused_and_verified_automatic_install_is_a_no_op() {
        assert_eq!(install_plan(true, true, false), Err(AlreadyHealthy));
        assert_eq!(install_plan(true, true, true), Err(AlreadyHealthy));
        assert_eq!(install_plan(false, true, true), Ok([].as_slice()));
    }

    #[test]
    fn replacing_a_package_checks_the_whole_write_before_restarting() {
        assert_eq!(
            install_plan(false, true, false),
            Ok([Put, Verify, Restart, WaitForBoot, ConfirmSupport].as_slice()),
        );
        for repair in [false, true] {
            assert_eq!(
                install_plan(repair, false, false),
                Ok([Put, Verify, Restart, ConfirmSupport].as_slice()),
            );
        }
    }

    #[test]
    fn retrying_a_verified_but_inactive_package_requires_explicit_recovery() {
        assert_eq!(install_plan(false, false, true), Err(RecoveryRequired));
        assert_eq!(
            install_plan(true, false, true),
            Ok([Retry, Restart, ConfirmSupport].as_slice()),
        );
    }
}
