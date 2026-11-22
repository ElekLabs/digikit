//! A failure at each bring-up step takes down exactly the steps done for that package, in reverse, and leaves the packages before it up.

use super::*;
use crate::State;
use dkrt_abi::provider::Limit;
use dkrt_fit::ledger::LedgerError;
use dkrt_sdk::E_MISSING;
use Call::*;

/// `ref` (1) is up; the log is cleared; loading `a` (2) fails with `fault`.
fn load_a_failing(fault: Call) -> (Vec<Call>, FakeManager, LifecycleError) {
    let (mut pm, probe) = world(&[reference(), user(2)]);
    pm.boot(&[id(1)]).unwrap();
    probe.clear();
    probe.fail(fault, E_MISSING);
    let e = err(pm.load(id(2)));
    (probe.calls(), pm, e)
}

fn failed(entering: State) -> LifecycleError {
    LifecycleError::Failed {
        package: id(2),
        entering,
        status: E_MISSING,
    }
}

fn assert_clean(pm: &FakeManager) {
    assert_eq!(live(pm, &[1, 2]), [1], "earlier packages stay up");
    assert_eq!(pm.state_of(id(2)), None);
    assert!(!pm.graph().node(id(2)).unwrap().is_pinned());
}

#[test]
fn place_fails_with_nothing_to_unwind() {
    let (calls, pm, e) = load_a_failing(Place(id(2)));
    assert_eq!(calls, [Place(id(2))]);
    assert_eq!(e, failed(State::Placed));
    assert_clean(&pm);
}

#[test]
fn link_fails_after_place_only() {
    let (calls, pm, e) = load_a_failing(Link(id(2)));
    assert_eq!(calls, [Place(id(2)), Link(id(2)), Free(id(2))]);
    assert_eq!(e, failed(State::Linked));
    assert_clean(&pm);
}

#[test]
fn start_fails_after_link_and_place() {
    let (calls, pm, e) = load_a_failing(Start(id(2)));
    assert_eq!(
        calls,
        [
            Place(id(2)),
            Link(id(2)),
            Start(id(2)),
            Unbind(id(2)),
            Free(id(2))
        ]
    );
    assert_eq!(e, failed(State::Started));
    assert_clean(&pm);
}

#[test]
fn install_fails_with_nothing_to_restore() {
    let (calls, pm, e) = load_a_failing(Install(id(2)));
    assert_eq!(
        calls,
        [
            Place(id(2)),
            Link(id(2)),
            Start(id(2)),
            Install(id(2)),
            Stop(id(2)),
            Unbind(id(2)),
            Free(id(2)),
        ]
    );
    assert_eq!(
        e,
        LifecycleError::Install {
            package: id(2),
            error: crate::InstallError::Backend(E_MISSING)
        }
    );
    assert_clean(&pm);
}

#[test]
fn attach_fails_after_install_so_the_entries_are_restored() {
    let (calls, pm, e) = load_a_failing(Attach(id(1), id(2)));
    assert_eq!(
        calls,
        [
            Place(id(2)),
            Link(id(2)),
            Start(id(2)),
            Install(id(2)),
            Activate(id(2)),
            Attach(id(1), id(2)),
            Deactivate(id(2)),
            Restore(id(2)),
            Stop(id(2)),
            Unbind(id(2)),
            Free(id(2)),
        ]
    );
    assert_eq!(e, failed(State::Active));
    assert_clean(&pm);
}

#[test]
fn a_failed_second_attach_detaches_the_first() {
    // module 3 needs services "ref" (1) and "other" (4); it attaches by ascending id: 1 then 4
    let pkgs = [
        reference(),
        Pkg::service(4, "other", V1),
        user(3).needs("other", V1),
    ];
    let (mut pm, probe) = world(&pkgs);
    pm.boot(&[id(1), id(4)]).unwrap();
    probe.clear();
    probe.fail(Attach(id(4), id(3)), E_MISSING);
    assert!(matches!(
        err(pm.load(id(3))),
        LifecycleError::Failed { package, .. } if package == id(3)
    ));
    let calls = probe.calls();
    assert!(calls.contains(&Detach(id(1), id(3))));
    assert!(!calls.contains(&Detach(id(4), id(3))));
    assert!(before(&calls, Detach(id(1), id(3)), Restore(id(3))));
    assert_eq!(live(&pm, &[1, 3, 4]), [1, 4]);
}

#[test]
fn a_failure_of_a_consumer_takes_down_the_service_this_load_started() {
    let (mut pm, probe) = world(&[reference(), user(2)]);
    probe.fail(Link(id(2)), E_MISSING);
    assert!(matches!(err(pm.load(id(2))), LifecycleError::Failed { .. }));
    assert_eq!(live(&pm, &[1, 2]), []);
    assert_eq!(pm.state_of(id(1)), None);
    let calls = probe.calls();
    assert!(before(&calls, Free(id(2)), Stop(id(1))));
    assert!(calls.contains(&Free(id(1))));
}

#[test]
fn a_failed_load_keeps_a_service_that_was_up_before_or_has_a_consumer_or_a_pin() {
    // 1 is up with consumer 3 (not pinned), so the failure of 2 leaves it alone
    let (mut pm, probe) = world(&[reference(), user(2), user(3)]);
    pm.load(id(3)).unwrap();
    probe.clear();
    probe.fail(Link(id(2)), E_MISSING);
    err(pm.load(id(2)));
    assert_eq!(live(&pm, &[1, 2, 3]), [1, 3]);
    assert!(!probe.calls().contains(&Stop(id(1))));
}

#[test]
fn a_service_chain_started_for_a_failed_consumer_goes_down_consumer_first() {
    let (mut pm, probe) = world(&[
        reference(),
        Pkg::service(4, "top", V1).needs("ref", V1),
        Pkg::module(5).needs("top", V1),
    ]);
    probe.fail(Link(id(5)), E_MISSING);
    err(pm.load(id(5)));
    assert!(pm.live().is_empty());
    let calls = probe.calls();
    assert!(before(&calls, Stop(id(4)), Stop(id(1))));
}

#[test]
fn a_busy_restore_during_the_unwind_is_finished_by_a_retry_that_then_frees_the_orphans() {
    // 2 fails to attach after its entries went in; the restore of 2 answers busy; 1 was started for it
    let (mut pm, probe) = world(&[reference(), user(2), user(3)]);
    probe.fail(Attach(id(1), id(2)), E_MISSING);
    probe.busy_restore(id(2), 1);
    assert!(matches!(err(pm.load(id(2))), LifecycleError::Failed { .. }));
    assert_eq!(pm.state_of(id(2)), Some(State::RestorePending));
    assert_eq!(pm.restore_pending(), Some(id(2)));
    // nothing else runs while the restore waits
    for e in [pm.load(id(3)), pm.reload(id(1)), pm.boot(&[id(3)])] {
        assert_eq!(err(e), LifecycleError::RestorePending { package: id(2) });
    }
    assert_eq!(
        err(pm.unload(id(1))),
        LifecycleError::RestorePending { package: id(2) }
    );
    probe.clear();
    let r = pm.retry_pending().unwrap();
    assert_eq!(taken(&r), ids(&[2, 1]));
    assert_eq!(
        probe.calls(),
        [
            Restore(id(2)),
            Stop(id(2)),
            Unbind(id(2)),
            Free(id(2)),
            Deactivate(id(1)),
            Restore(id(1)),
            Stop(id(1)),
            Unbind(id(1)),
            Free(id(1)),
        ],
        "the retry asks nothing again: 2 was deactivated before its restore went busy"
    );
    assert_eq!(pm.restore_pending(), None);
    assert!(pm.live().is_empty());
    // later operations work
    pm.load(id(3)).unwrap();
    assert_eq!(live(&pm, &[1, 2, 3]), [1, 3]);
    assert_eq!(pm.state_of(id(2)), None);
}

#[test]
fn a_busy_restore_in_a_boot_refuses_what_is_left_and_the_retry_finishes_it() {
    let (mut pm, probe) = world(&[reference(), user(2), user(3)]);
    probe.fail(Attach(id(1), id(2)), E_MISSING);
    probe.busy_restore(id(2), 1);
    let report = pm.boot(&[id(2), id(3)]).unwrap();
    assert_eq!(brought(&report), ids(&[1]));
    let refused: Vec<_> = report.refused().iter().map(|r| r.package).collect();
    assert_eq!(refused, ids(&[2, 3]));
    assert_eq!(
        report.refused()[1].reason,
        LifecycleError::RestorePending { package: id(2) }
    );
    assert_eq!(
        report.pending(),
        Some(id(2)),
        "the boot says a take-down waits"
    );
    pm.retry_pending().unwrap();
    assert!(pm.live().is_empty());
    pm.boot(&[id(2), id(3)]).unwrap();
    assert_eq!(live(&pm, &[1, 2, 3]), [1, 2, 3]);
}

#[test]
fn a_claim_that_fails_releases_what_the_package_took() {
    // `check_fit` refuses such a load before it starts; the step itself is checked here
    let (mut pm, probe) = world(&[reference(), user(2).wants(1, "slots", 5)]);
    pm.boot(&[id(1)]).unwrap();
    probe.clear();
    assert!(matches!(
        pm.bring_up_package(id(2)),
        Err(LifecycleError::Short { package, .. }) if package == id(2)
    ));
    assert_eq!(
        probe.calls(),
        [
            Place(id(2)),
            Link(id(2)),
            Start(id(2)),
            Stop(id(2)),
            Unbind(id(2)),
            Free(id(2))
        ]
    );
    assert_eq!(pm.state_of(id(2)), None);
}

#[test]
fn a_start_that_declares_other_pools_than_the_header_is_refused_and_unwound() {
    let (mut pm, probe) = world(&[reference()]);
    pm.host.declares(id(1), std::vec![(7, Limit::Fixed(4))]);
    assert_eq!(
        err(pm.load(id(1))),
        LifecycleError::Declaration { package: id(1) }
    );
    assert_eq!(
        probe.calls(),
        [
            Place(id(1)),
            Link(id(1)),
            Start(id(1)),
            Stop(id(1)),
            Unbind(id(1)),
            Free(id(1))
        ]
    );
    assert!(pm.live().is_empty());
}

#[test]
fn a_pool_declared_twice_is_a_ledger_refusal() {
    let (mut pm, _) = world(&[reference()]);
    pm.declare_resources(id(1)).unwrap();
    assert_eq!(
        err(pm.load(id(1))),
        LifecycleError::Ledger {
            package: id(1),
            error: LedgerError::Duplicate
        }
    );
}

#[test]
fn a_busy_orphan_restore_after_a_boot_is_reported_as_pending() {
    // 2 fails to attach and unwinds; the restore of the orphan 1, started for it, is busy
    let (mut pm, probe) = world(&[reference(), user(2)]);
    probe.fail(Attach(id(1), id(2)), E_MISSING);
    probe.busy_restore(id(1), 1);
    let report = pm.boot(&[id(2)]).unwrap();
    assert_eq!(report.pending(), Some(id(1)));
    assert_eq!(pm.restore_pending(), Some(id(1)));
    pm.retry_pending().unwrap();
    assert!(pm.live().is_empty());
}

#[test]
fn activate_fails_after_install_so_the_entries_are_restored_and_nothing_was_attached() {
    let (calls, pm, e) = load_a_failing(Activate(id(2)));
    assert_eq!(
        calls,
        [
            Place(id(2)),
            Link(id(2)),
            Start(id(2)),
            Install(id(2)),
            Activate(id(2)),
            Restore(id(2)),
            Stop(id(2)),
            Unbind(id(2)),
            Free(id(2)),
        ]
    );
    assert_eq!(e, failed(State::Activated));
    assert_clean(&pm);
}
