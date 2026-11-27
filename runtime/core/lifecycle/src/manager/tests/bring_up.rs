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
            ImportState(id(1)),
            Install(id(1)),
            Activate(id(1)),
            Attach(id(1)),
            Place(id(2)),
            Link(id(2)),
            Start(id(2)),
            ImportState(id(2)),
            Install(id(2)),
            Activate(id(2)),
            Attach(id(2)),
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
    assert!(probe.calls().contains(&Attach(id(3))));
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

#[test]
fn boot_brings_every_service_up_before_the_first_module() {
    // module 0 needs `other` (package 4); module 2 needs `ref` (package 1): ascending roots would put module 0 and its service before the service of module 2
    let pkgs = [
        reference(),
        Pkg::service(4, "other", V1),
        Pkg::module(0).needs("other", V1),
        user(2),
    ];
    let (mut pm, probe) = world(&pkgs);
    let report = pm.boot(&[id(0), id(2)]).unwrap();
    assert_eq!(brought(&report), ids(&[4, 1, 0, 2]));
    let calls = probe.calls();
    assert!(position(&calls, Start(id(1))) < position(&calls, Start(id(0))));
    assert!(position(&calls, Start(id(4))) < position(&calls, Start(id(0))));
}

#[test]
fn the_dsp_part_starts_when_its_package_is_active_before_the_next_is_placed_and_is_not_awaited() {
    let pkgs = [reference().with_dsp(), user(2).with_dsp(), user(3)];
    let (mut pm, probe) = world(&pkgs);
    let report = pm.boot(&[id(2), id(3)]).unwrap();
    assert_eq!(report.dsp_pending().as_slice(), ids(&[1, 2]));
    let calls = probe.calls();
    assert!(position(&calls, StartDsp(id(1))) > position(&calls, Activate(id(1))));
    assert!(
        position(&calls, StartDsp(id(1))) < position(&calls, Place(id(2))),
        "one reservation at a time"
    );
    assert!(
        position(&calls, StartDsp(id(2))) > position(&calls, Attach(id(2))),
        "a module starts it once attached"
    );
    assert_eq!(pm.state_of(id(2)), Some(crate::State::Active));
    assert!(
        !calls.contains(&StartDsp(id(3))),
        "a package with no DSP part is not reported"
    );
}

#[test]
fn a_refused_package_starts_no_dsp_part() {
    let pkgs = [reference(), user(2).with_dsp()];
    let (mut pm, probe) = world(&pkgs);
    probe.fail(Link(id(2)), dkrt_sdk::E_MISSING);
    let report = pm.boot(&[id(2)]).unwrap();
    assert!(report.dsp_pending().is_empty());
    assert!(!probe.calls().iter().any(|c| matches!(c, StartDsp(_))));
}

#[test]
fn a_failed_dsp_part_takes_down_the_closure_and_leaves_the_services_it_needs() {
    // service 1 has consumers 2 and 3; its DSP part fails: 2 and 3 go with it. Module 4 needs another service and stays.
    let pkgs = [
        reference().with_dsp(),
        user(2),
        user(3),
        Pkg::service(5, "other", V1),
        Pkg::module(4).needs("other", V1),
    ];
    let (mut pm, _) = world(&pkgs);
    pm.boot(&[id(2), id(3), id(4)]).unwrap();
    let r = pm.take_down_closure(id(1)).unwrap();
    assert_eq!(taken(&r), ids(&[2, 3, 1]));
    assert_eq!(live(&pm, &[1, 2, 3, 4, 5]), [4, 5]);
    // a module whose DSP part fails leaves the service it needs up
    let (mut pm, _) = world(&[reference(), user(2).with_dsp()]);
    pm.boot(&[id(2)]).unwrap();
    assert_eq!(taken(&pm.take_down_closure(id(2)).unwrap()), ids(&[2]));
    assert_eq!(live(&pm, &[1, 2]), [1]);
}

#[test]
fn refresh_graph_reads_the_manifest_again_and_keeps_the_pins() {
    let (mut pm, _) = world(&[reference(), user(2)]);
    pm.boot(&[id(2)]).unwrap();
    assert!(pm.load(id(3)).is_err(), "package 3 is not in the graph");
    pm.manifest_mut().put(&user(3));
    pm.refresh_graph().unwrap();
    assert!(
        pm.graph().node(id(2)).unwrap().is_pinned(),
        "the boot's pin stays"
    );
    pm.load(id(3)).unwrap();
    assert_eq!(live(&pm, &[1, 2, 3]), [1, 2, 3]);
    // a replaced table is read: package 3 needs nothing now
    pm.manifest_mut().put(&Pkg::module(3));
    pm.refresh_graph().unwrap();
    assert_eq!(pm.graph().services_of(id(3)).count(), 0);
}

#[test]
fn the_manifest_admits_each_package_in_bring_up_order_and_a_refusal_takes_its_consumers_with_it() {
    let pkgs = [reference(), user(2), user(3)];
    let (mut pm, probe) = world(&pkgs);
    pm.boot(&[id(2), id(3)]).unwrap();
    assert_eq!(probe.admitted(), ids(&[1, 2, 3]));
    // a refused module is refused alone, and takes nothing of the others
    let (mut pm, probe) = world(&pkgs);
    probe.fail(Admit(id(2)), dkrt_sdk::E_PLAN_OWNER);
    let report = pm.boot(&[id(2), id(3)]).unwrap();
    assert_eq!(brought(&report), ids(&[1, 3]));
    assert!(matches!(
        report.refused(),
        [r] if r.package == id(2) && r.reason == LifecycleError::Admission { package: id(2), status: dkrt_sdk::E_PLAN_OWNER }
    ));
    assert!(
        !probe.calls().contains(&Place(id(2))),
        "a refused package is not placed"
    );
    // a refused service refuses its consumers without asking about them, and nothing is placed
    let (mut pm, probe) = world(&pkgs);
    probe.fail(Admit(id(1)), dkrt_sdk::E_PLAN_KEY);
    let report = pm.boot(&[id(2), id(3)]).unwrap();
    assert!(report.brought_up().is_empty());
    let refused: Vec<_> = report.refused().iter().map(|r| r.package).collect();
    assert_eq!(refused, ids(&[1, 2, 3]));
    assert!(matches!(
        report.refused()[1].reason,
        LifecycleError::NeedRefused { .. }
    ));
    assert_eq!(probe.admitted(), ids(&[1]));
    assert!(!probe.calls().iter().any(|c| matches!(c, Place(_))));
}

#[test]
fn the_host_hears_services_up_once_per_boot_between_the_services_and_the_first_module_and_never_for_a_load(
) {
    let pkgs = [
        reference(),
        Pkg::service(4, "other", V1),
        Pkg::module(0).needs("other", V1),
        user(2),
    ];
    let (mut pm, probe) = world(&pkgs);
    pm.boot(&[id(0), id(2)]).unwrap();
    let calls = probe.calls();
    assert_eq!(calls.iter().filter(|c| **c == ServicesUp).count(), 1);
    assert!(position(&calls, Activate(id(1))) < position(&calls, ServicesUp));
    assert!(position(&calls, Activate(id(4))) < position(&calls, ServicesUp));
    assert!(position(&calls, ServicesUp) < position(&calls, Place(id(0))));
    // a boot with services only (or nothing that comes up) tells the host at its end
    let (mut pm, probe) = world(&pkgs);
    pm.boot(&[id(1)]).unwrap();
    assert_eq!(
        probe.calls().iter().filter(|c| **c == ServicesUp).count(),
        1
    );
    // a load needs no numbering: the live job has its own
    let (mut pm, probe) = world(&pkgs);
    pm.load(id(2)).unwrap();
    assert!(!probe.calls().contains(&ServicesUp));
}

#[test]
fn a_module_with_a_soft_need_on_a_refused_service_comes_up() {
    // service 4 fails to link, and service 6 is refused by the rules of the core; modules 5 and 7 only have soft needs of them, module 8 a hard one
    let pkgs = [
        Pkg::service(4, "top", V1),
        Pkg::module(5).soft_needs("top", V1),
        Pkg::service(6, "side", V1),
        Pkg::module(7).soft_needs("side", V1),
        Pkg::module(8).needs("top", V1),
    ];
    let (mut pm, probe) = world(&pkgs);
    probe.fail(Link(id(4)), dkrt_sdk::E_MISSING);
    probe.fail(Admit(id(6)), dkrt_sdk::E_PLAN_OWNER);
    let report = pm.boot(&[id(5), id(7), id(8)]).unwrap();
    assert_eq!(brought(&report), ids(&[5, 7]));
    let refused: Vec<_> = report.refused().iter().map(|r| r.package).collect();
    assert_eq!(
        refused,
        ids(&[6, 4, 8]),
        "the hard need of 8 is refused with its service, the soft ones are not"
    );
    assert!(!probe.calls().contains(&Place(id(8))));
}

#[test]
fn a_package_with_keys_for_a_kind_comes_up_after_the_package_that_declares_it() {
    const KIND: u32 = 90;
    // module 0 has keys for the kind, module 3 declares it: the owner is first, whatever the ids say
    let (mut pm, _) = world(&[
        Pkg::module(0).emits_kind(KIND),
        Pkg::module(3).owns_kind(KIND),
    ]);
    let report = pm.boot(&[id(0), id(3)]).unwrap();
    assert_eq!(brought(&report), ids(&[3, 0]));
    // a service that declares the kind comes up for a module that has keys for it and needs nothing else
    let (mut pm, _) = world(&[
        Pkg::service(1, "kinds", V1).owns_kind(KIND),
        Pkg::module(0).emits_kind(KIND),
    ]);
    let report = pm.boot(&[id(0)]).unwrap();
    assert_eq!(brought(&report), ids(&[1, 0]));
    // when the owner is not up, the package with keys is still tried: the rules of the core say the kind has no owner
    let (mut pm, probe) = world(&[
        Pkg::service(1, "kinds", V1).owns_kind(KIND),
        Pkg::module(0).emits_kind(KIND),
    ]);
    probe.fail(Admit(id(1)), dkrt_sdk::E_PLAN_OWNER);
    let report = pm.boot(&[id(0)]).unwrap();
    assert_eq!(brought(&report), ids(&[0]));
    assert!(probe.admitted().contains(&id(0)));
}
