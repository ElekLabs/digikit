use super::*;
use crate::State;
use Call::*;

#[test]
fn load_brings_services_up_first_and_each_package_through_every_step() {
    let (mut pm, probe) = world(&[reference(), user(2)]);
    let r = pm.load(id(2)).unwrap();
    assert_eq!(brought(&r), ids(&[1, 2]));
    assert_eq!(
        probe.calls(),
        [
            Place(id(1)),
            Link(id(1)),
            Start(id(1)),
            Install(id(1)),
            Activate(id(1)),
            Place(id(2)),
            Link(id(2)),
            Start(id(2)),
            Install(id(2)),
            Activate(id(2)),
            Attach(id(1), id(2)),
        ]
    );
    assert_eq!(pm.state_of(id(1)), Some(State::Active));
    assert_eq!(pm.state_of(id(2)), Some(State::Active));
    assert_eq!(live(&pm, &[1, 2]), [1, 2]);
}

#[test]
fn load_skips_services_that_are_up() {
    let (mut pm, probe) = world(&[reference(), user(2), user(3)]);
    pm.load(id(2)).unwrap();
    probe.clear();
    let r = pm.load(id(3)).unwrap();
    assert_eq!(brought(&r), ids(&[3]));
    assert!(!probe.calls().contains(&Place(id(1))));
    assert!(probe.calls().contains(&Attach(id(1), id(3))));
}

#[test]
fn load_of_a_live_package_is_refused() {
    let (mut pm, _) = world(&[reference(), user(2)]);
    pm.load(id(2)).unwrap();
    assert_eq!(
        err(pm.load(id(2))),
        LifecycleError::AlreadyLive { package: id(2) }
    );
}

#[test]
fn boot_brings_up_every_root_in_graph_order() {
    let (mut pm, probe) = world(&[user(3), reference(), user(2)]);
    let r = pm.boot(&[id(3), id(2)]).unwrap();
    assert_eq!(brought(&r), ids(&[1, 2, 3]));
    let starts: Vec<Call> = probe
        .calls()
        .into_iter()
        .filter(|c| matches!(c, Start(_)))
        .collect();
    assert_eq!(starts, [Start(id(1)), Start(id(2)), Start(id(3))]);
}

#[test]
fn an_unmet_need_is_refused_before_anything_is_placed() {
    let (mut pm, probe) = world(&[user(2)]);
    assert!(matches!(
        err(pm.load(id(2))),
        LifecycleError::Graph(crate::Refusal::Unmet { .. })
    ));
    assert!(probe.calls().is_empty());
}

#[test]
fn boot_continues_past_a_failure_and_refuses_the_consumers_of_the_failed_package() {
    // 4 is a service that fails to link; 5 needs it; 2 needs only `ref`
    let (mut pm, probe) = world(&[
        reference(),
        Pkg::service(4, "top", V1),
        Pkg::module(5).needs("top", V1),
        user(2),
    ]);
    probe.fail(Link(id(4)), dkrt_sdk::E_MISSING);
    let report = pm.boot(&[id(2), id(5)]).unwrap();
    assert_eq!(brought(&report), ids(&[1, 2]));
    let refused: Vec<_> = report.refused().iter().map(|r| r.package).collect();
    assert_eq!(refused, ids(&[4, 5]), "the cause comes first");
    assert!(matches!(
        report.refused()[0].reason,
        LifecycleError::Failed { package, .. } if package == id(4)
    ));
    assert_eq!(
        report.refused()[1].reason,
        LifecycleError::NeedRefused {
            package: id(5),
            service: id(4)
        }
    );
    assert!(!probe.calls().contains(&Place(id(5))));
    assert_eq!(live(&pm, &[1, 2, 4, 5]), [1, 2]);
    assert!(pm.graph().node(id(2)).unwrap().is_pinned());
    assert!(!pm.graph().node(id(5)).unwrap().is_pinned());
}

#[test]
fn boot_takes_down_the_service_of_a_refused_consumer_and_keeps_the_pinned_roots() {
    // 2 and 3 need `ref` (1); 3 fails; 1 stays because 2 needs it. 6 needs `other` (4); 6 fails; 4 had no other consumer and goes
    let pkgs = [
        reference(),
        Pkg::service(4, "other", V1),
        user(2),
        user(3),
        Pkg::module(6).needs("other", V1),
    ];
    let (mut pm, probe) = world(&pkgs);
    probe.fail(Link(id(3)), dkrt_sdk::E_MISSING);
    probe.fail(Link(id(6)), dkrt_sdk::E_MISSING);
    let report = pm.boot(&[id(2), id(3), id(6)]).unwrap();
    assert_eq!(live(&pm, &[1, 2, 3, 4, 6]), [1, 2]);
    assert_eq!(taken(&report), ids(&[4]));
    let refused: Vec<_> = report.refused().iter().map(|r| r.package).collect();
    assert_eq!(refused, ids(&[3, 6]));
    // a pinned service root stays up without a consumer
    let (mut pm, probe) = world(&pkgs);
    probe.fail(Link(id(6)), dkrt_sdk::E_MISSING);
    pm.boot(&[id(4), id(6)]).unwrap();
    assert_eq!(live(&pm, &[4, 6]), [4]);
}

#[test]
fn boot_refuses_a_root_the_graph_refuses_and_brings_up_the_others() {
    let (mut pm, _) = world(&[reference(), user(2), Pkg::module(3).needs("ghost", V1)]);
    let report = pm.boot(&[id(3), id(2)]).unwrap();
    assert_eq!(brought(&report), ids(&[1, 2]));
    assert!(matches!(
        report.refused(),
        [r] if r.package == id(3) && matches!(r.reason, LifecycleError::Graph(crate::Refusal::Unmet { .. }))
    ));
}
