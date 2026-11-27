use super::*;
use crate::State;
use Call::*;

#[test]
fn reloading_a_service_reloads_its_consumers_and_ends_with_the_same_live_set() {
    let (mut pm, probe) = world(&[reference(), user(2), user(3)]);
    pm.boot(&[id(1), id(2), id(3)]).unwrap();
    probe.clear();
    let r = pm.reload(id(1)).unwrap();
    assert_eq!(taken(&r), ids(&[2, 3, 1]));
    assert_eq!(brought(&r), ids(&[1, 2, 3]));
    assert_eq!(live(&pm, &[1, 2, 3]), [1, 2, 3]);
    let calls = probe.calls();
    assert!(before(&calls, Free(id(1)), Place(id(1))));
    assert!(before(&calls, Detach(id(2)), Stop(id(1))));
    assert!(before(&calls, Start(id(1)), Attach(id(2))));
    for n in [1, 2, 3] {
        assert_eq!(pm.state_of(id(n)), Some(State::Active));
    }
}

#[test]
fn a_reload_tells_the_host_the_services_are_up_before_the_first_consumer_comes_back() {
    let (mut pm, probe) = world(&[reference(), user(2), user(3)]);
    pm.boot(&[id(1), id(2), id(3)]).unwrap();
    probe.clear();
    pm.reload(id(1)).unwrap();
    let calls = probe.calls();
    assert_eq!(calls.iter().filter(|c| **c == ServicesUp).count(), 1);
    assert!(before(&calls, Activate(id(1)), ServicesUp));
    assert!(
        before(&calls, ServicesUp, Place(id(2))),
        "the consumers start with numbers the new service gave"
    );
}

#[test]
fn reloading_a_module_leaves_its_service_alone() {
    let (mut pm, probe) = world(&[reference(), user(2), user(3)]);
    pm.load(id(2)).unwrap();
    probe.clear();
    let r = pm.reload(id(2)).unwrap();
    assert_eq!(taken(&r), ids(&[2]));
    assert_eq!(brought(&r), ids(&[2]));
    let calls = probe.calls();
    assert!(!calls.contains(&Stop(id(1))));
    assert!(!calls.contains(&Place(id(1))));
    assert_eq!(live(&pm, &[1, 2, 3]), [1, 2]);
}

#[test]
fn reload_keeps_pins() {
    let (mut pm, _) = world(&[reference(), user(2)]);
    pm.boot(&[id(1)]).unwrap();
    pm.load(id(2)).unwrap();
    pm.reload(id(1)).unwrap();
    assert!(pm.graph().node(id(1)).unwrap().is_pinned());
    // the service stays up when its consumer goes, because the pin survived the reload
    pm.unload(id(2)).unwrap();
    assert_eq!(live(&pm, &[1]), [1]);
}

#[test]
fn a_service_that_cannot_stop_refuses_the_reload_and_nothing_is_touched() {
    let (mut pm, probe) = world(&[reference(), user(2)]);
    pm.boot(&[id(1), id(2)]).unwrap();
    probe.clear();
    probe.fail(CanStop(id(1)), dkrt_sdk::E_BUSY);
    assert!(matches!(
        err(pm.reload(id(1))),
        LifecycleError::CannotStop { .. }
    ));
    assert_eq!(probe.calls(), [CanStop(id(2)), CanStop(id(1))]);
}

#[test]
fn a_busy_restore_stops_the_reload_and_a_retry_completes_it() {
    let (mut pm, probe) = world(&[reference(), user(2)]);
    pm.boot(&[id(1), id(2)]).unwrap();
    probe.busy_restore(id(2), 1);
    assert_eq!(
        err(pm.reload(id(1))),
        LifecycleError::Busy {
            package: id(2),
            task: None
        }
    );
    assert_eq!(live(&pm, &[1, 2]), [1, 2], "nothing came back or went on");
    assert!(matches!(
        err(pm.unload(id(1))),
        LifecycleError::RestorePending { .. }
    ));
    let r = pm.retry_pending().unwrap();
    assert_eq!(brought(&r), ids(&[1, 2]));
    assert_eq!(live(&pm, &[1, 2]), [1, 2]);
    assert_eq!(pm.restore_pending(), None);
}

#[test]
fn reloading_a_package_that_is_not_up_is_refused() {
    let (mut pm, _) = world(&[reference(), user(2)]);
    assert_eq!(
        err(pm.reload(id(2))),
        LifecycleError::NotLive { package: id(2) }
    );
}

#[test]
fn a_failure_while_bringing_the_set_back_leaves_the_rest_down() {
    let (mut pm, probe) = world(&[reference(), user(2), user(3)]);
    pm.boot(&[id(1), id(2), id(3)]).unwrap();
    probe.fail(Link(id(2)), dkrt_sdk::E_MISSING);
    assert!(matches!(
        err(pm.reload(id(1))),
        LifecycleError::Failed { .. }
    ));
    assert_eq!(live(&pm, &[1, 2, 3]), [1]);
    assert_eq!(pm.restore_pending(), None);
}

#[test]
fn a_reload_that_would_not_fit_touches_nothing() {
    let (mut pm, probe) = world(&[reference(), user(2).wants(1, "slots", 2)]);
    pm.boot(&[id(1), id(2)]).unwrap();
    // the next build of 2 wants more than the pool has
    pm.manifest.replace_claims(&user(2).wants(1, "slots", 5));
    probe.clear();
    assert!(matches!(
        err(pm.reload(id(2))),
        LifecycleError::Short { package, .. } if package == id(2)
    ));
    assert!(probe.calls().is_empty(), "not even can_stop was asked");
    assert_eq!(live(&pm, &[1, 2]), [1, 2]);
    assert_eq!(pm.restore_pending(), None);
}

#[test]
fn a_reload_counts_the_claims_of_its_closure_as_released() {
    // 2 holds all 4 slots: its reload fits only because its own claim goes first
    let (mut pm, _) = world(&[reference(), user(2).wants(1, "slots", 4)]);
    pm.boot(&[id(1), id(2)]).unwrap();
    pm.reload(id(2)).unwrap();
    assert_eq!(live(&pm, &[1, 2]), [1, 2]);
    // a service reload re-declares its pools and its consumers claim again
    pm.reload(id(1)).unwrap();
    assert_eq!(live(&pm, &[1, 2]), [1, 2]);
}

#[test]
fn a_reload_still_counts_the_claims_of_packages_outside_its_closure() {
    let (mut pm, probe) = world(&[
        reference(),
        user(2).wants(1, "slots", 2),
        user(3).wants(1, "slots", 2),
    ]);
    pm.boot(&[id(1), id(2), id(3)]).unwrap();
    pm.manifest.replace_claims(&user(2).wants(1, "slots", 3));
    probe.clear();
    assert!(matches!(
        err(pm.reload(id(2))),
        LifecycleError::Short { .. }
    ));
    assert!(probe.calls().is_empty());
}

#[test]
fn a_reload_exports_state_after_the_restore_and_before_stop_and_imports_after_start() {
    let (mut pm, probe) = world(&[reference(), user(2)]);
    pm.boot(&[id(1), id(2)]).unwrap();
    probe.clear();
    pm.reload(id(1)).unwrap();
    let calls = probe.calls();
    for n in [1, 2] {
        assert_eq!(
            calls.iter().filter(|c| **c == ExportState(id(n))).count(),
            1
        );
        assert!(before(&calls, Restore(id(n)), ExportState(id(n))));
        assert!(before(&calls, ExportState(id(n)), Stop(id(n))));
        assert!(before(&calls, Start(id(n)), ImportState(id(n))));
        assert!(before(&calls, ImportState(id(n)), Install(id(n))));
    }
    // the new copy of the service takes the state after the old one gave it
    assert!(before(&calls, ExportState(id(1)), ImportState(id(1))));
}

#[test]
fn an_unload_and_a_plain_load_export_nothing() {
    let (mut pm, probe) = world(&[reference(), user(2)]);
    pm.boot(&[id(1), id(2)]).unwrap();
    pm.unload(id(2)).unwrap();
    pm.load(id(2)).unwrap();
    pm.unload(id(2)).unwrap();
    assert!(!probe.calls().iter().any(|c| matches!(c, ExportState(_))));
    assert!(probe.calls().contains(&ImportState(id(2))));
}

#[test]
fn a_busy_restore_in_a_reload_exports_once_after_the_retry() {
    let (mut pm, probe) = world(&[reference(), user(2)]);
    pm.boot(&[id(1), id(2)]).unwrap();
    probe.clear();
    probe.busy_restore(id(2), 1);
    assert!(matches!(err(pm.reload(id(1))), LifecycleError::Busy { .. }));
    assert!(
        !probe.calls().contains(&ExportState(id(2))),
        "the busy restore stopped before the export"
    );
    pm.retry_pending().unwrap();
    let calls = probe.calls();
    assert_eq!(
        calls.iter().filter(|c| **c == ExportState(id(2))).count(),
        1
    );
    assert!(before(&calls, ExportState(id(2)), Stop(id(2))));
}

/// The copy of `p` that failed to take its state is stopped, then freed, after the import (the old copy's own stop and free came before it).
fn assert_unwound_after_import(calls: &[Call], p: PackageId) {
    let at = calls
        .iter()
        .position(|c| *c == ImportState(p))
        .expect("an import");
    let after = &calls[at..];
    let stop = after
        .iter()
        .position(|c| *c == Stop(p))
        .expect("a stop after the failed import");
    let free = after
        .iter()
        .position(|c| *c == Free(p))
        .expect("a free after the failed import");
    assert!(stop < free, "{calls:?}");
}

#[test]
fn a_failed_import_fails_the_start_and_leaves_the_rest_down() {
    let (mut pm, probe) = world(&[reference(), user(2), user(3)]);
    pm.boot(&[id(1), id(2), id(3)]).unwrap();
    probe.clear();
    probe.fail(ImportState(id(1)), dkrt_sdk::E_CONTRACT);
    assert!(matches!(
        err(pm.reload(id(1))),
        LifecycleError::Failed { package, status, .. }
            if package == id(1) && status == dkrt_sdk::E_CONTRACT
    ));
    assert_eq!(
        live(&pm, &[1, 2, 3]),
        [],
        "the service and its consumers stay down"
    );
    assert!(!probe.calls().contains(&Install(id(2))));
    assert_eq!(pm.restore_pending(), None);
    let calls = probe.calls();
    assert_unwound_after_import(&calls, id(1));
}

#[test]
fn a_busy_import_is_not_retried_it_is_a_clean_failure() {
    // a copy that has just started has nothing in flight, so busy is not a wait the manager can offer: the start fails like any other, the closure goes down whole
    let (mut pm, probe) = world(&[reference(), user(2)]);
    pm.boot(&[id(1), id(2)]).unwrap();
    probe.clear();
    probe.fail(ImportState(id(1)), dkrt_sdk::E_BUSY);
    assert!(matches!(
        err(pm.reload(id(1))),
        LifecycleError::Failed { package, status, .. }
            if package == id(1) && status == dkrt_sdk::E_BUSY
    ));
    let calls = probe.calls();
    assert_eq!(
        calls.iter().filter(|c| **c == ImportState(id(1))).count(),
        1,
        "asked once"
    );
    assert_unwound_after_import(&calls, id(1));
    assert_eq!(live(&pm, &[1, 2]), []);
    assert_eq!(
        pm.restore_pending(),
        None,
        "nothing waits: the unwind is whole"
    );
}
