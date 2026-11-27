//! A pending attach (a service takes a payload over more than one UI pass): the states `Attaching` and `Detaching`, `PackageManager::step`, the failure and the timeout of an
//! attach, a take-down that waits at its busy detach.
use super::*;
use crate::fake::ATTACH_LIMIT;
use crate::State;
use dkrt_sdk::E_INIT;
use Call::*;

#[test]
fn a_pending_attach_is_stepped_to_active_one_pass_at_a_time() {
    let (mut pm, probe) = world(&[reference(), user(2)]);
    probe.pending_attach(id(2), 2);
    let r = pm.load(id(2)).unwrap();
    // up and live, not active: the report says it is still attaching
    assert_eq!(brought(&r), ids(&[1, 2]));
    assert_eq!(r.attaching().as_slice(), ids(&[2]));
    assert_eq!(pm.state_of(id(2)), Some(State::Attaching));
    assert!(pm.live().contains(id(2)));
    // the services of the walk are the same on each call
    probe.clear();
    let r = pm.step();
    assert_eq!(r.attaching().as_slice(), ids(&[2]));
    assert!(r.brought_up().is_empty());
    assert_eq!(probe.walk_of(id(2)), ids(&[1]));
    assert_eq!(pm.state_of(id(2)), Some(State::Attaching));
    let r = pm.step();
    assert_eq!(brought(&r), ids(&[2]));
    assert!(r.attaching().is_empty());
    assert_eq!(pm.state_of(id(2)), Some(State::Active));
    assert_eq!(probe.calls(), [Attach(id(2)), Attach(id(2))]);
    // nothing waits any more: a pass does nothing
    probe.clear();
    assert_eq!(pm.step(), crate::Report::default());
    assert!(probe.calls().is_empty());
}

#[test]
fn a_package_that_is_attaching_does_not_block_the_next_bring_up() {
    let (mut pm, probe) = world(&[reference(), user(2), user(3)]);
    probe.pending_attach(id(2), 3);
    pm.load(id(2)).unwrap();
    let r = pm.load(id(3)).unwrap();
    assert_eq!(brought(&r), ids(&[3]));
    assert_eq!(pm.state_of(id(3)), Some(State::Active));
    assert_eq!(pm.state_of(id(2)), Some(State::Attaching));
}

#[test]
fn a_refused_attach_takes_the_package_down_inside_the_manager_and_leaves_its_neighbours() {
    let (mut pm, probe) = world(&[reference(), user(2), user(3)]);
    probe.pending_attach(id(2), 1);
    pm.load(id(2)).unwrap();
    pm.load(id(3)).unwrap();
    probe.clear();
    // the second call of the attach is refused: nothing is attached, so no `Detach`
    probe.fail(Attach(id(2)), E_INIT);
    let r = pm.step();
    let refused = r.refused();
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].package, id(2));
    assert_eq!(
        refused[0].reason,
        LifecycleError::Failed {
            package: id(2),
            entering: State::Active,
            status: E_INIT
        }
    );
    assert_eq!(taken(&r), ids(&[2]));
    assert!(!pm.live().contains(id(2)));
    // the module that came up beside it and the service stay
    assert_eq!(live(&pm, &[1, 3]), [1, 3]);
    let calls = probe.calls();
    assert!(!calls.contains(&Detach(id(2))), "{calls:?}");
    assert!(calls.contains(&Stop(id(2))));
    assert!(pm.state_of(id(2)).is_none());
}

#[test]
fn a_refused_attach_takes_the_consumers_of_the_package_down_with_it_but_not_its_services() {
    let (mut pm, probe) = world(&[
        reference(),
        Pkg::service(2, "top", V1).needs("ref", V1),
        Pkg::module(3).needs("top", V1),
    ]);
    // the service attaches to `ref` over two passes; its consumer is brought up after it and is attached at once
    probe.pending_attach(id(2), 1);
    pm.load(id(3)).unwrap();
    assert_eq!(pm.state_of(id(2)), Some(State::Attaching));
    assert_eq!(pm.state_of(id(3)), Some(State::Active));
    probe.fail(Attach(id(2)), E_INIT);
    let r = pm.step();
    assert_eq!(taken(&r), ids(&[3, 2]));
    assert_eq!(live(&pm, &[1, 2, 3]), [1]);
    assert_eq!(r.refused()[0].package, id(2));
}

#[test]
fn an_attach_that_does_not_end_in_time_takes_the_closure_down_and_detaches_it() {
    let (mut pm, probe) = world(&[reference(), user(2)]);
    probe.pending_attach(id(2), 1000);
    pm.load(id(2)).unwrap();
    probe.clear();
    for pass in 0..ATTACH_LIMIT {
        let r = pm.step();
        assert_eq!(r.attaching().as_slice(), ids(&[2]), "pass {pass}");
    }
    // the next pass is over the limit
    let r = pm.step();
    assert_eq!(
        r.refused()[0].reason,
        LifecycleError::AttachTimeout { package: id(2) }
    );
    assert_eq!(taken(&r), ids(&[2]));
    assert!(!pm.live().contains(id(2)));
    // the services held part of it: they are told to give it back before the stop
    let calls = probe.calls();
    assert!(before(&calls, Detach(id(2)), Stop(id(2))));
    assert!(!pm.graph().node(id(2)).unwrap().is_pinned());
}

#[test]
fn a_detach_that_answers_pending_keeps_the_take_down_waiting_and_a_retry_resumes_at_it() {
    let (mut pm, probe) = world(&[reference(), user(2), user(3)]);
    pm.boot(&[id(1), id(2)]).unwrap();
    probe.clear();
    probe.pending_detach(id(2), 2);
    assert_eq!(
        err(pm.unload(id(2))),
        LifecycleError::Busy { package: id(2) }
    );
    assert_eq!(pm.state_of(id(2)), Some(State::Detaching));
    assert_eq!(pm.restore_pending(), Some(id(2)));
    assert!(pm.live().contains(id(2)));
    // nothing was undone past the detach: still public, entries in, not stopped
    let calls = probe.calls();
    assert!(!calls.contains(&Deactivate(id(2))), "{calls:?}");
    assert!(!calls.contains(&Restore(id(2))));
    // nothing else comes or goes meanwhile
    assert_eq!(
        err(pm.load(id(3))),
        LifecycleError::RestorePending { package: id(2) }
    );
    assert_eq!(
        err(pm.unload(id(1))),
        LifecycleError::RestorePending { package: id(2) }
    );
    // the second detach still answers pending, the third is done
    assert_eq!(
        err(pm.retry_pending()),
        LifecycleError::Busy { package: id(2) }
    );
    probe.clear();
    let r = pm.retry_pending().unwrap();
    assert_eq!(taken(&r), ids(&[2]));
    let calls = probe.calls();
    assert_eq!(
        &calls[..3],
        [Detach(id(2)), Deactivate(id(2)), Restore(id(2))]
    );
    assert_eq!(live(&pm, &[1, 2]), [1]);
    assert!(pm.restore_pending().is_none());
}

#[test]
fn a_take_down_the_manager_started_waits_at_its_pending_detach_and_a_step_finishes_it() {
    let (mut pm, probe) = world(&[reference(), user(2)]);
    probe.pending_attach(id(2), 1000);
    pm.load(id(2)).unwrap();
    probe.pending_detach(id(2), 1);
    for _ in 0..ATTACH_LIMIT {
        pm.step();
    }
    // over the limit: the closure goes down, and the detach answers pending once
    let r = pm.step();
    assert_eq!(
        r.refused()[0].reason,
        LifecycleError::AttachTimeout { package: id(2) }
    );
    assert!(r.taken_down().is_empty());
    assert_eq!(r.pending(), Some(id(2)));
    assert_eq!(pm.state_of(id(2)), Some(State::Detaching));
    // no other operation while it waits
    assert!(matches!(
        err(pm.load(id(1))),
        LifecycleError::RestorePending { .. } | LifecycleError::AlreadyLive { .. }
    ));
    // the next pass resumes at the detach: it is done now and the take-down ends
    let r = pm.step();
    assert_eq!(taken(&r), ids(&[2]));
    assert!(r.pending().is_none());
    assert!(!pm.live().contains(id(2)));
}

#[test]
fn a_second_failed_attach_waits_for_the_take_down_that_is_pending() {
    let (mut pm, probe) = world(&[reference(), user(2), user(3)]);
    probe.pending_attach(id(2), 1000);
    probe.pending_attach(id(3), 1000);
    pm.load(id(2)).unwrap();
    pm.load(id(3)).unwrap();
    for _ in 0..ATTACH_LIMIT {
        pm.step();
    }
    // both are over the limit on this pass; package 2's take-down waits at its detach, so package 3 stays attaching
    probe.pending_detach(id(2), 1);
    let r = pm.step();
    assert_eq!(r.refused().len(), 1);
    assert_eq!(r.refused()[0].package, id(2));
    assert_eq!(r.pending(), Some(id(2)));
    assert_eq!(r.attaching().as_slice(), ids(&[3]));
    assert_eq!(pm.state_of(id(3)), Some(State::Attaching));
    // the next pass finishes package 2's take-down and then gives up package 3
    let r = pm.step();
    assert_eq!(taken(&r), ids(&[2, 3]));
    assert_eq!(r.refused()[0].package, id(3));
    assert_eq!(live(&pm, &[1, 2, 3]), [1]);
}

#[test]
fn a_caller_can_give_up_a_pending_attach() {
    let (mut pm, probe) = world(&[reference(), user(2)]);
    assert!(matches!(
        pm.cancel_attach(id(2), LifecycleError::AttachTimeout { package: id(2) }),
        Err(LifecycleError::NotLive { .. })
    ));
    probe.pending_attach(id(2), 1000);
    pm.load(id(2)).unwrap();
    let r = pm
        .cancel_attach(id(2), LifecycleError::AttachTimeout { package: id(2) })
        .unwrap();
    assert_eq!(taken(&r), ids(&[2]));
    assert_eq!(r.refused()[0].package, id(2));
    assert!(!pm.live().contains(id(2)));
}

#[test]
fn an_attach_that_fails_while_another_take_down_is_pending_is_not_attached_again() {
    let (mut pm, probe) = world(&[reference(), user(2), user(3)]);
    probe.pending_attach(id(3), 1);
    pm.load(id(3)).unwrap();
    pm.load(id(2)).unwrap();
    // package 2's take-down waits at its detach
    probe.pending_detach(id(2), 1);
    pm.unload(id(2)).ok();
    probe.clear();
    probe.fail(Attach(id(3)), E_INIT);
    let r = pm.step();
    assert!(
        r.refused().is_empty(),
        "the failed attach waits for the take-down"
    );
    assert_eq!(pm.state_of(id(3)), Some(State::Attaching));
    let attaches = |probe: &crate::fake::Probe| {
        probe
            .calls()
            .iter()
            .filter(|c| **c == Attach(id(3)))
            .count()
    };
    assert_eq!(attaches(&probe), 1);
    // the other take-down ends; package 3 is taken down without a second attach call
    pm.retry_pending().unwrap();
    let r = pm.step();
    assert_eq!(attaches(&probe), 1, "{:?}", probe.calls());
    assert_eq!(r.refused().len(), 1);
    assert_eq!(r.refused()[0].package, id(3));
    assert!(pm.state_of(id(3)).is_none());
}
