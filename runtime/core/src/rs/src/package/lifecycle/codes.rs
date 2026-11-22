//! The load status that the boot record and the live job report for a manager refusal: the codes the loader answered before the manager existed, so Transfer and the menu
//! show the same for the same cause.
use dkrt_lifecycle::{InstallError, LifecycleError, Refusal};
use dkrt_sdk::*;

/// The status of a package that `PackageManager` refused or stopped, `reason`.
///
/// - a status of a step (`place`, `link`, `start`, `activate`, `attach`) is the loader's own code, unchanged;
/// - the rules of the core refused the package (`Manifest::admit`): their status (`E_PLAN_*`);
/// - a service that this package needs was refused: `E_MISSING` (the code of a module whose provider did not load);
/// - an unmet or incompatible need in the graph: `E_MISSING`, `E_CONTRACT`; a cycle (packages that need each other, or declare the kinds the other has keys for):
///   `E_MISSING`; a second service of one name: `E_PLAN_OWNER` (the codes of the planner for the same causes);
/// - the claims do not fit, or a table is full: `E_PLAN_FIT`; the declarations differ from the header: `E_DECL_KIND`;
/// - a refusal to stop or a busy restore: `E_BUSY`.
pub(crate) fn code_of(reason: &LifecycleError) -> i32 {
    // if-let chains, not a `match`: a dense match becomes a PC-relative jump table, which is not ColdFire code
    if let LifecycleError::Failed { status, .. } = *reason {
        status
    } else if let LifecycleError::Admission { status, .. } = *reason {
        status
    } else if let LifecycleError::NeedRefused { .. } = *reason {
        E_MISSING
    } else if let LifecycleError::Graph(g) = *reason {
        graph_code(g)
    } else if let LifecycleError::Declaration { .. } = *reason {
        E_DECL_KIND
    } else if let LifecycleError::Install { error, .. } = *reason {
        install_code(error)
    } else if let LifecycleError::CannotStop { status, .. } = *reason {
        status
    } else if let LifecycleError::Short { .. } = *reason {
        E_PLAN_FIT
    } else if let LifecycleError::Ledger { .. } = *reason {
        E_PLAN_FIT
    } else if let LifecycleError::CoreResource(_) = *reason {
        E_PLAN_FIT
    } else if let LifecycleError::NotLive { .. } = *reason {
        E_NOT_FOUND
    } else if let LifecycleError::AlreadyLive { .. } = *reason {
        E_LOADED
    } else {
        // `Busy`, `RestoreFailed`, `RestorePending`, `NothingPending`
        E_BUSY
    }
}

fn graph_code(g: Refusal) -> i32 {
    if let Refusal::Unmet { .. } = g {
        E_MISSING
    } else if let Refusal::Incompatible { .. } = g {
        E_CONTRACT
    } else if let Refusal::Cycle { .. } = g {
        E_MISSING
    } else if let Refusal::DuplicateService { .. } = g {
        E_PLAN_OWNER
    } else if let Refusal::UnknownPackage { .. } = g {
        E_NOT_FOUND
    } else if let Refusal::BadTable { .. } = g {
        E_FORMAT
    } else {
        E_PLAN_FIT
    }
}

fn install_code(e: InstallError) -> i32 {
    if let InstallError::Backend(status) = e {
        status
    } else {
        E_PLAN_FIT
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dkrt_abi::package::PackageId;
    use dkrt_lifecycle::State;

    const P: PackageId = PackageId(1);

    #[test]
    fn a_failed_step_keeps_the_loaders_code_and_the_other_causes_get_the_planners_codes() {
        let failed = LifecycleError::Failed { package: P, entering: State::Started, status: E_INIT };
        assert_eq!(code_of(&failed), E_INIT);
        assert_eq!(code_of(&LifecycleError::NeedRefused { package: P, service: PackageId(9) }), E_MISSING);
        assert_eq!(code_of(&LifecycleError::Admission { package: P, status: E_PLAN_KEY }), E_PLAN_KEY);
        assert_eq!(code_of(&LifecycleError::Graph(Refusal::Unmet { package: P, service: 7 })), E_MISSING);
        assert_eq!(code_of(&LifecycleError::Graph(Refusal::Incompatible { package: P, service: 7 })), E_CONTRACT);
        assert_eq!(code_of(&LifecycleError::Graph(Refusal::Cycle { package: P })), E_MISSING);
        assert_eq!(code_of(&LifecycleError::Graph(Refusal::DuplicateService { package: P, service: 7 })), E_PLAN_OWNER);
        assert_eq!(code_of(&LifecycleError::Declaration { package: P }), E_DECL_KIND);
        assert_eq!(code_of(&LifecycleError::Install { package: P, error: InstallError::Backend(E_IMPORT) }), E_IMPORT);
        assert_eq!(code_of(&LifecycleError::Install { package: P, error: InstallError::TableFull }), E_PLAN_FIT);
        assert_eq!(code_of(&LifecycleError::CannotStop { package: P, status: E_RELOAD_ROUTED }), E_RELOAD_ROUTED);
        assert_eq!(code_of(&LifecycleError::Busy { package: P }), E_BUSY);
    }
}
