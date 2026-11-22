use super::*;
use crate::State;
use dkrt_sdk::E_BUSY;
use Call::*;

#[test]
fn take_down_detaches_before_restore_restores_before_stop_and_unbinds_before_free() {
    let (mut pm, probe) = world(&[reference(), user(2)]);
    pm.load(id(2)).unwrap();
    probe.clear();
    let r = pm.unload(id(2)).unwrap();
    assert_eq!(taken(&r), ids(&[2, 1]));
    assert_eq!(
        probe.calls(),
        [
            CanStop(id(2)),
            CanStop(id(1)),
            Detach(id(2)),
            Deactivate(id(2)),
            Restore(id(2)),
            Stop(id(2)),
            Unbind(id(2)),
            Free(id(2)),
            Detach(id(1)),
            Deactivate(id(1)),
            Restore(id(1)),
            Stop(id(1)),
            Unbind(id(1)),
            Free(id(1)),
        ]
    );
    assert!(pm.live().is_empty());
}

#[test]
fn can_stop_is_asked_of_every_package_of_the_closure_before_anything_is_touched() {
    let (mut pm, probe) = world(&[
        reference(),
        Pkg::service(4, "top", V1).needs("ref", V1),
        Pkg::module(5).needs("top", V1),
    ]);
    pm.load(id(5)).unwrap();
    probe.clear();
    pm.unload(id(5)).unwrap();
    let calls = probe.calls();
    let first_touch = position(&calls, Detach(id(5)));
    assert_eq!(
        &calls[..first_touch],
        [CanStop(id(5)), CanStop(id(4)), CanStop(id(1))]
    );
}

#[test]
fn a_service_that_cannot_stop_refuses_the_unload_and_nothing_is_touched() {
    let (mut pm, probe) = world(&[reference(), user(2)]);
    pm.load(id(2)).unwrap();
    probe.clear();
    probe.fail(CanStop(id(1)), E_BUSY);
    assert_eq!(
        err(pm.unload(id(2))),
        LifecycleError::CannotStop {
            package: id(1),
            status: E_BUSY
        }
    );
    assert_eq!(probe.calls(), [CanStop(id(2)), CanStop(id(1))]);
    assert_eq!(live(&pm, &[1, 2]), [1, 2]);
    assert_eq!(pm.state_of(id(2)), Some(State::Active));
}

#[test]
fn the_last_consumer_takes_an_unpinned_service_with_it_but_not_before() {
    let (mut pm, _) = world(&[reference(), user(2), user(3)]);
    pm.load(id(2)).unwrap();
    pm.load(id(3)).unwrap();
    // `load` pins 2 and 3 (a request names them); the service came as a need, so it is unpinned
    assert_eq!(taken(&pm.unload(id(2)).unwrap()), ids(&[2]));
    assert_eq!(live(&pm, &[1, 3]), [1, 3]);
    assert_eq!(taken(&pm.unload(id(3)).unwrap()), ids(&[3, 1]));
    assert!(pm.live().is_empty());
}

#[test]
fn a_pinned_service_stays() {
    let (mut pm, probe) = world(&[reference(), user(2)]);
    pm.boot(&[id(1), id(2)]).unwrap();
    probe.clear();
    assert_eq!(taken(&pm.unload(id(2)).unwrap()), ids(&[2]));
    assert!(!probe.calls().contains(&Stop(id(1))));
    assert_eq!(live(&pm, &[1]), [1]);
}

#[test]
fn unloading_a_service_takes_its_consumers_down_first() {
    let (mut pm, _) = world(&[reference(), user(2), user(3)]);
    pm.boot(&[id(1), id(2), id(3)]).unwrap();
    assert_eq!(taken(&pm.unload(id(1)).unwrap()), ids(&[2, 3, 1]));
    assert!(pm.live().is_empty());
}

#[test]
fn unloading_a_package_that_is_not_up_is_refused() {
    let (mut pm, probe) = world(&[reference(), user(2)]);
    assert_eq!(
        err(pm.unload(id(2))),
        LifecycleError::NotLive { package: id(2) }
    );
    assert!(probe.calls().is_empty());
}

#[test]
fn a_busy_restore_keeps_the_package_pending_and_a_retry_finishes_it() {
    let (mut pm, probe) = world(&[reference(), user(2).wants(1, "slots", 2), user(3)]);
    pm.boot(&[id(1), id(2)]).unwrap();
    probe.clear();
    probe.busy_restore(id(2), 1);
    assert_eq!(
        err(pm.unload(id(2))),
        LifecycleError::Busy { package: id(2) }
    );
    assert_eq!(pm.state_of(id(2)), Some(State::RestorePending));
    assert_eq!(pm.restore_pending(), Some(id(2)));
    assert!(pm.live().contains(id(2)));
    let calls = probe.calls();
    assert!(calls.contains(&Detach(id(2))));
    assert!(
        !calls.contains(&Stop(id(2))),
        "not stopped while its entries are in firmware"
    );
    // the claims were released before the restore
    let mut rows = [dkrt_fit::ledger::PoolReport::EMPTY; 4];
    pm.ledger().report(&mut rows);
    assert_eq!(rows[0].used(), 0);

    // nothing else comes or goes while the restore waits
    assert_eq!(
        err(pm.load(id(3))),
        LifecycleError::RestorePending { package: id(2) }
    );
    probe.clear();
    let r = pm.retry_pending().unwrap();
    assert_eq!(taken(&r), ids(&[2]));
    assert_eq!(
        probe.calls(),
        [Restore(id(2)), Stop(id(2)), Unbind(id(2)), Free(id(2))],
        "the retry restarts at the restore and asks nothing again"
    );
    assert_eq!(pm.restore_pending(), None);
    assert_eq!(live(&pm, &[1, 2]), [1]);
}

#[test]
fn a_retry_finishes_the_rest_of_the_closure() {
    let (mut pm, probe) = world(&[reference(), user(2), user(3)]);
    pm.boot(&[id(1), id(2), id(3)]).unwrap();
    probe.busy_restore(id(2), 1);
    assert!(matches!(err(pm.unload(id(1))), LifecycleError::Busy { .. }));
    // another unload is not allowed while this one waits
    assert_eq!(
        err(pm.unload(id(3))),
        LifecycleError::RestorePending { package: id(2) }
    );
    let r = pm.retry_pending().unwrap();
    assert_eq!(taken(&r), ids(&[2, 3, 1]));
    assert!(pm.live().is_empty());
}

#[test]
fn a_restore_that_fails_for_another_reason_is_reported_as_such_and_stays_pending() {
    let (mut pm, probe) = world(&[reference(), user(2)]);
    pm.boot(&[id(1), id(2)]).unwrap();
    probe.fail(Restore(id(2)), dkrt_sdk::E_MISSING);
    assert!(matches!(
        err(pm.unload(id(2))),
        LifecycleError::RestoreFailed { package, .. } if package == id(2)
    ));
    assert_eq!(pm.state_of(id(2)), Some(State::RestorePending));
    pm.retry_pending().unwrap();
    assert_eq!(live(&pm, &[1, 2]), [1]);
}

#[test]
fn a_package_that_comes_back_after_unload_goes_through_all_steps_again() {
    let (mut pm, probe) = world(&[reference(), user(2).wants(1, "slots", 4)]);
    pm.load(id(2)).unwrap();
    pm.unload(id(2)).unwrap();
    probe.clear();
    pm.load(id(2)).unwrap();
    let places = probe
        .calls()
        .iter()
        .filter(|c| matches!(c, Place(_)))
        .count();
    assert_eq!(places, 2, "the service came back as a need");
}

#[test]
fn retry_pending_finishes_a_busy_restore_of_the_first_orphan() {
    let (mut pm, probe) = world(&[reference(), user(2), user(3)]);
    pm.boot(&[id(1), id(2), id(3)]).unwrap();
    probe.busy_restore(id(2), 1);
    assert_eq!(
        err(pm.unload(id(1))),
        LifecycleError::Busy { package: id(2) }
    );
    assert_eq!(taken(&pm.retry_pending().unwrap()), ids(&[2, 3, 1]));
    assert!(pm.live().is_empty());
    assert_eq!(pm.restore_pending(), None);
}

#[test]
fn retry_pending_finishes_a_busy_restore_of_the_second_orphan() {
    // the take-down order is 2, 3, 1: the restore of 3 is busy, not the first package
    let (mut pm, probe) = world(&[reference(), user(2), user(3)]);
    pm.boot(&[id(1), id(2), id(3)]).unwrap();
    probe.busy_restore(id(3), 1);
    assert_eq!(
        err(pm.unload(id(1))),
        LifecycleError::Busy { package: id(3) }
    );
    assert_eq!(live(&pm, &[1, 2, 3]), [1, 3]);
    // the other operations still wait, whatever package they name
    assert_eq!(
        err(pm.unload(id(3))),
        LifecycleError::RestorePending { package: id(3) }
    );
    assert_eq!(taken(&pm.retry_pending().unwrap()), ids(&[2, 3, 1]));
    assert!(pm.live().is_empty());
    assert_eq!(pm.restore_pending(), None);
}

#[test]
fn retry_pending_can_be_busy_again_and_then_succeeds() {
    let (mut pm, probe) = world(&[reference(), user(2), user(3)]);
    pm.boot(&[id(1), id(2), id(3)]).unwrap();
    probe.busy_restore(id(3), 2);
    assert!(matches!(err(pm.unload(id(1))), LifecycleError::Busy { .. }));
    assert_eq!(
        err(pm.retry_pending()),
        LifecycleError::Busy { package: id(3) }
    );
    assert_eq!(pm.restore_pending(), Some(id(3)));
    assert_eq!(taken(&pm.retry_pending().unwrap()), ids(&[2, 3, 1]));
    assert!(pm.live().is_empty());
}

#[test]
fn retry_pending_without_a_pending_take_down_is_refused() {
    let (mut pm, probe) = world(&[reference(), user(2)]);
    pm.boot(&[id(1), id(2)]).unwrap();
    probe.clear();
    assert_eq!(err(pm.retry_pending()), LifecycleError::NothingPending);
    assert!(probe.calls().is_empty());
}

#[test]
fn a_module_that_cannot_stop_refuses_the_unload_like_a_service_does() {
    let (mut pm, probe) = world(&[reference(), user(2)]);
    pm.load(id(2)).unwrap();
    probe.clear();
    probe.fail(CanStop(id(2)), E_BUSY);
    assert_eq!(
        err(pm.unload(id(2))),
        LifecycleError::CannotStop {
            package: id(2),
            status: E_BUSY
        }
    );
    assert_eq!(probe.calls(), [CanStop(id(2))]);
    assert_eq!(live(&pm, &[1, 2]), [1, 2]);
}

#[test]
fn the_closure_take_down_leaves_the_orphaned_service_and_the_pin_and_asks_nothing() {
    let (mut pm, probe) = world(&[reference(), user(2)]);
    pm.load(id(2)).unwrap();
    probe.clear();
    probe.fail(CanStop(id(2)), E_BUSY);
    let r = pm.take_down_closure(id(2)).unwrap();
    assert_eq!(taken(&r), ids(&[2]));
    assert_eq!(
        live(&pm, &[1, 2]),
        [1],
        "the service stays up with no consumer"
    );
    assert!(!probe.calls().iter().any(|c| matches!(c, CanStop(_))));
    assert!(
        pm.graph().node(id(2)).unwrap().is_pinned(),
        "the pin stays for the load that follows"
    );
    // the second half of a split reload: the module comes back, the service is not touched
    probe.clear();
    pm.load(id(2)).unwrap();
    assert!(!probe.calls().contains(&Place(id(1))));
    assert_eq!(live(&pm, &[1, 2]), [1, 2]);
}

#[test]
fn check_stop_asks_the_closure_and_touches_nothing() {
    let (mut pm, probe) = world(&[reference(), user(2)]);
    pm.load(id(2)).unwrap();
    probe.clear();
    assert_eq!(pm.check_stop(id(2)), Ok(()));
    assert_eq!(probe.calls(), [CanStop(id(2))]);
    probe.fail(CanStop(id(1)), E_BUSY);
    assert!(matches!(
        pm.check_stop(id(1)),
        Err(LifecycleError::CannotStop { package, .. }) if package == id(1)
    ));
    assert_eq!(
        pm.check_stop(id(9)),
        Err(LifecycleError::NotLive { package: id(9) })
    );
    assert_eq!(live(&pm, &[1, 2]), [1, 2]);
}
