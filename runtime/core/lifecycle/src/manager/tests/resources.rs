use super::*;
use dkrt_fit::ledger::PoolReport;
use Call::*;

fn rows(pm: &FakeManager) -> Vec<PoolReport> {
    let mut out = [PoolReport::EMPTY; 20];
    let n = pm.ledger().report(&mut out);
    out[..n].to_vec()
}

#[test]
fn a_package_that_does_not_fit_is_refused_alone_and_the_others_come_up() {
    // two users want 3 of 4 slots each: the second does not fit, the first does
    let (mut pm, probe) = world(&[
        reference(),
        user(2).wants(1, "slots", 3),
        user(3).wants(1, "slots", 3),
    ]);
    let report = pm.boot(&[id(2), id(3)]).unwrap();
    assert_eq!(brought(&report), ids(&[1, 2]));
    let [refused] = report.refused() else {
        panic!("{:?}", report.refused())
    };
    assert_eq!(refused.package, id(3));
    // its own need against what the first user left of the pool of 4
    let LifecycleError::Short { package, short } = refused.reason else {
        panic!("{:?}", refused.reason)
    };
    assert_eq!(package, id(3));
    assert_eq!((short.need(), short.free()), (3, 1));
    assert!(!probe.calls().contains(&Place(id(3))));
    assert_eq!(live(&pm, &[1, 2, 3]), [1, 2]);
    assert_eq!(rows(&pm)[0].used(), 3);
    assert!(pm.graph().node(id(2)).unwrap().is_pinned());
    assert!(!pm.graph().node(id(3)).unwrap().is_pinned());
}

#[test]
fn a_service_that_does_not_fit_is_refused_with_its_consumers_and_nothing_is_placed_for_them() {
    let (mut pm, probe) = world(&[
        reference(),
        Pkg::service(4, "big", V1).wants(1, "slots", 9),
        Pkg::module(5).needs("big", V1),
        user(2),
    ]);
    let report = pm.boot(&[id(2), id(5)]).unwrap();
    assert_eq!(brought(&report), ids(&[1, 2]));
    let refused: Vec<_> = report.refused().iter().map(|r| r.package).collect();
    assert_eq!(refused, ids(&[4, 5]));
    assert_eq!(
        report.refused()[1].reason,
        LifecycleError::NeedRefused {
            package: id(5),
            service: id(4)
        }
    );
    assert!(!probe.calls().contains(&Place(id(4))));
    assert!(!probe.calls().contains(&Place(id(5))));
}

#[test]
fn a_service_started_only_for_a_package_that_does_not_fit_is_not_placed() {
    let (mut pm, probe) = world(&[reference(), user(2).wants(1, "slots", 5)]);
    assert!(matches!(err(pm.load(id(2))), LifecycleError::Short { .. }));
    assert!(probe.calls().is_empty());
    assert!(pm.live().is_empty());
    assert!(rows(&pm).is_empty(), "the plan leaves no pool and no claim");
}

#[test]
fn a_consumer_cannot_claim_a_pool_its_service_did_not_declare() {
    let (mut pm, probe) = world(&[reference(), user(2).wants(1, "ghost", 1)]);
    let e = err(pm.load(id(2)));
    let LifecycleError::Short { package, short } = e else {
        panic!("{e:?}")
    };
    assert_eq!(package, id(2));
    assert_eq!(short.free(), 0);
    assert!(probe.calls().is_empty());
}

#[test]
fn claims_are_held_while_up_and_released_with_the_pools_after_unload() {
    let (mut pm, _) = world(&[reference(), user(2).wants(1, "slots", 3)]);
    pm.boot(&[id(1)]).unwrap();
    pm.load(id(2)).unwrap();
    assert_eq!(rows(&pm)[0].used(), 3);
    pm.unload(id(2)).unwrap();
    assert_eq!(rows(&pm)[0].used(), 0, "the consumer's claims are released");
    pm.unload(id(1)).unwrap();
    assert!(rows(&pm).is_empty(), "the service's pools are withdrawn");
}

#[test]
fn units_come_back_for_the_next_consumer() {
    let (mut pm, _) = world(&[
        reference(),
        user(2).wants(1, "slots", 3),
        user(3).wants(1, "slots", 2),
    ]);
    pm.boot(&[id(1)]).unwrap();
    pm.load(id(2)).unwrap();
    assert!(matches!(err(pm.load(id(3))), LifecycleError::Short { .. }));
    pm.unload(id(2)).unwrap();
    pm.load(id(3)).unwrap();
    assert_eq!(rows(&pm)[0].used(), 2);
}

#[test]
fn a_service_declares_its_pools_before_its_consumer_claims() {
    let (mut pm, probe) = world(&[reference(), user(2).wants(1, "slots", 1)]);
    pm.load(id(2)).unwrap();
    assert!(before(&probe.calls(), Start(id(1)), Start(id(2))));
    assert_eq!(rows(&pm)[0].used(), 1);
}
