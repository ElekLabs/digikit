extern crate std;

use super::*;
use crate::{Api, LiveSet, Need, Node, PackageId};
use std::vec::Vec;

fn id(n: u8) -> PackageId {
    PackageId(n)
}

fn ids(v: &[u8]) -> Vec<PackageId> {
    v.iter().map(|&n| id(n)).collect()
}

fn live(v: &[u8]) -> LiveSet {
    LiveSet::of(&ids(v))
}

/// Service names are small numbers in these tests.
fn service(n: u8, name: u32, version: u32) -> Node {
    Node::new(id(n)).providing(Api::new(name, version))
}

fn module(n: u8) -> Node {
    Node::new(id(n))
}

fn needs(node: Node, service: u32, min_version: u32) -> Node {
    node.needing(Need::new(service, min_version))
}

const REF: u32 = 100;
const BASE: u32 = 101;
const V1: u32 = 0x010000;

fn graph(nodes: &[Node]) -> PackageGraph {
    PackageGraph::from_nodes(nodes).unwrap()
}

fn down(g: &PackageGraph, target: u8, l: &[u8]) -> Vec<PackageId> {
    g.take_down_order(id(target), &live(l))
        .into_iter()
        .collect()
}

fn up(g: &PackageGraph, roots: &[u8], l: &[u8]) -> Result<Vec<PackageId>, Refusal> {
    g.bring_up_order(&ids(roots), &live(l))
        .map(|s| s.into_iter().collect())
}

#[test]
fn unloading_last_consumer_unloads_unpinned_service() {
    let g = graph(&[
        service(1, REF, V1),
        needs(module(2), REF, V1),
        needs(module(3), REF, V1),
    ]);
    assert_eq!(down(&g, 2, &[1, 2, 3]), ids(&[2]));
    assert_eq!(down(&g, 3, &[1, 3]), ids(&[3, 1]));
}

#[test]
fn consumers_go_down_first_transitively() {
    // module 3 needs service 2, which needs service 1; module 4 needs service 1 too
    let g = graph(&[
        service(1, REF, V1),
        needs(service(2, BASE, V1), REF, V1),
        needs(module(3), BASE, V1),
        needs(module(4), REF, V1),
    ]);
    // unloading service 1 takes down everything that needs it, each before what it needs
    assert_eq!(down(&g, 1, &[1, 2, 3, 4]), ids(&[3, 2, 4, 1]));
    // unloading a mid service takes its consumer first and leaves service 1 to module 4
    assert_eq!(down(&g, 2, &[1, 2, 3, 4]), ids(&[3, 2]));
}

#[test]
fn orphan_chain_goes_down() {
    // module 3 -> service 2 -> service 1: only module 3 is unloaded, both services were only for it
    let g = graph(&[
        service(1, REF, V1),
        needs(service(2, BASE, V1), REF, V1),
        needs(module(3), BASE, V1),
    ]);
    assert_eq!(down(&g, 3, &[1, 2, 3]), ids(&[3, 2, 1]));
    // the same when the lower service has the higher id
    let g = graph(&[
        service(2, REF, V1),
        needs(service(1, BASE, V1), REF, V1),
        needs(module(3), BASE, V1),
    ]);
    assert_eq!(down(&g, 3, &[1, 2, 3]), ids(&[3, 1, 2]));
    // a service another live package still needs stays
    let g = graph(&[
        service(1, REF, V1),
        needs(service(2, BASE, V1), REF, V1),
        needs(module(3), BASE, V1),
        needs(module(4), REF, V1),
    ]);
    assert_eq!(down(&g, 3, &[1, 2, 3, 4]), ids(&[3, 2]));
}

#[test]
fn pinned_service_stays() {
    let g = graph(&[
        service(1, REF, V1).pin(),
        needs(service(2, BASE, V1), REF, V1),
        needs(module(3), BASE, V1),
    ]);
    // the pinned service is not an orphan, the unpinned one above it is
    assert_eq!(down(&g, 3, &[1, 2, 3]), ids(&[3, 2]));
    // an explicit unload of a pinned service still goes
    assert_eq!(down(&g, 1, &[1, 2, 3]), ids(&[3, 2, 1]));
}

#[test]
fn a_service_nobody_took_down_for_stays() {
    // service 5 is live and idle; unloading module 3 does not touch it
    let g = graph(&[
        service(1, REF, V1),
        service(5, BASE, V1),
        needs(module(3), REF, V1),
    ]);
    assert_eq!(down(&g, 3, &[1, 3, 5]), ids(&[3, 1]));
    assert!(down(&g, 3, &[1, 5]).is_empty());
}

#[test]
fn unmet_need_is_refused() {
    let g = graph(&[service(1, REF, V1), needs(module(2), BASE, V1)]);
    assert_eq!(
        up(&g, &[2], &[]),
        Err(Refusal::Unmet {
            package: id(2),
            service: BASE
        })
    );
    // a service that needs something unmet refuses its consumer's load and names the service package
    let g = graph(&[
        needs(service(1, REF, V1), BASE, V1),
        needs(module(2), REF, V1),
    ]);
    assert_eq!(
        up(&g, &[2], &[]),
        Err(Refusal::Unmet {
            package: id(1),
            service: BASE
        })
    );
    assert_eq!(
        up(&g, &[9], &[]),
        Err(Refusal::UnknownPackage { package: id(9) })
    );
}

#[test]
fn incompatible_version_is_refused() {
    let g = graph(&[service(1, REF, 0x010100), needs(module(2), REF, 0x010200)]);
    let want = Err(Refusal::Incompatible {
        package: id(2),
        service: REF,
    });
    assert_eq!(up(&g, &[2], &[]), want);
    // a newer major is no match either, and a live provider is checked as well
    let g = graph(&[service(1, REF, 0x020000), needs(module(2), REF, 0x010200)]);
    assert_eq!(up(&g, &[2], &[1]), want);
    // a newer minor of the same major is
    let g = graph(&[service(1, REF, 0x010300), needs(module(2), REF, 0x010200)]);
    assert_eq!(up(&g, &[2], &[]).unwrap(), ids(&[1, 2]));
}

#[test]
fn two_majors_of_one_service_resolve_by_version() {
    let g = graph(&[
        service(1, REF, 0x010000),
        service(2, REF, 0x020000),
        needs(module(3), REF, 0x020000),
    ]);
    assert_eq!(up(&g, &[3], &[]).unwrap(), ids(&[2, 3]));
    assert!(
        PackageGraph::from_nodes(&[service(1, REF, 0x010000), service(2, REF, 0x010200)]).is_err()
    );
}

#[test]
fn a_service_that_needs_itself_is_refused_at_construction() {
    assert_eq!(
        PackageGraph::from_nodes(&[needs(service(1, REF, V1), REF, V1)]).unwrap_err(),
        Refusal::Cycle { package: id(1) }
    );
}

#[test]
fn services_that_need_each_other_are_refused_at_construction() {
    let two = [
        needs(service(1, REF, V1), BASE, V1),
        needs(service(2, BASE, V1), REF, V1),
    ];
    assert!(matches!(
        PackageGraph::from_nodes(&two),
        Err(Refusal::Cycle { .. })
    ));
    // a consumer outside the cycle does not hide it
    let three = [two[0], two[1], needs(module(3), REF, V1)];
    assert!(matches!(
        PackageGraph::from_nodes(&three),
        Err(Refusal::Cycle { .. })
    ));
    // a longer ring
    let ring = [
        needs(service(1, REF, V1), BASE, V1),
        needs(service(2, BASE, V1), 0x33, V1),
        needs(service(3, 0x33, V1), REF, V1),
    ];
    assert!(matches!(
        PackageGraph::from_nodes(&ring),
        Err(Refusal::Cycle { .. })
    ));
}

#[test]
fn orders_are_deterministic() {
    // two services and three modules, nodes given in a scrambled order
    let nodes = [
        needs(needs(module(9), BASE, V1), REF, V1),
        service(7, BASE, V1),
        needs(module(2), REF, V1),
        service(4, REF, V1),
        needs(module(5), BASE, V1),
    ];
    let g = graph(&nodes);
    let mut reversed = nodes;
    reversed.reverse();
    let r = graph(&reversed);
    for roots in [[9, 5, 2], [2, 9, 5], [5, 2, 9]] {
        let order = up(&g, &roots, &[]).unwrap();
        // services in ascending id before the first consumer, roots in ascending id
        assert_eq!(order, ids(&[4, 2, 7, 5, 9]));
        assert_eq!(up(&r, &roots, &[]).unwrap(), order);
    }
    assert_eq!(down(&g, 4, &[2, 4, 5, 7, 9]), down(&r, 4, &[9, 7, 5, 4, 2]));
    assert_eq!(down(&g, 4, &[2, 4, 5, 7, 9]), ids(&[2, 9, 4]));
}

#[test]
fn live_packages_are_skipped_and_services_come_first() {
    let g = graph(&[
        service(1, REF, V1),
        needs(service(2, BASE, V1), REF, V1),
        needs(module(3), BASE, V1),
    ]);
    assert_eq!(up(&g, &[3], &[]).unwrap(), ids(&[1, 2, 3]));
    assert_eq!(up(&g, &[3], &[1]).unwrap(), ids(&[2, 3]));
    assert_eq!(up(&g, &[3], &[1, 2, 3]).unwrap(), ids(&[]));
}

#[test]
fn services_and_consumers_are_listed() {
    let g = graph(&[
        service(1, REF, V1),
        service(2, BASE, V1),
        needs(needs(module(3), BASE, V1), REF, V1),
        needs(module(4), REF, V1),
    ]);
    assert_eq!(g.services_of(id(3)).collect::<Vec<_>>(), ids(&[1, 2]));
    assert_eq!(g.consumers_of(id(1)).collect::<Vec<_>>(), ids(&[3, 4]));
    assert_eq!(g.consumers_of(id(2)).collect::<Vec<_>>(), ids(&[3]));
    assert_eq!(g.services_of(id(2)).count(), 0);
}

#[test]
fn a_graph_over_the_bounds_is_refused() {
    let many: Vec<Node> = (0..=MAX_PACKAGES as u8).map(module).collect();
    assert_eq!(
        PackageGraph::from_nodes(&many).unwrap_err(),
        Refusal::TooManyPackages
    );
    assert_eq!(
        PackageGraph::from_nodes(&[module(1), module(1)]).unwrap_err(),
        Refusal::DuplicatePackage { package: id(1) }
    );
    let mut n = module(1);
    for s in 0..=MAX_NEEDS as u32 {
        n = needs(n, s, V1);
    }
    assert_eq!(
        PackageGraph::from_nodes(&[n]).unwrap_err(),
        Refusal::TooManyNeeds { package: id(1) }
    );
}

#[test]
fn a_soft_need_orders_after_a_present_provider_and_is_met_by_none() {
    let soft = Node::new(id(2)).needing(Need::soft(REF, V1));
    let g = graph(&[service(1, REF, V1), soft]);
    assert_eq!(up(&g, &[2], &[]).unwrap(), ids(&[1, 2]));
    // the soft edge counts as a consumer: the provider goes down with its last consumer
    assert_eq!(down(&g, 2, &[1, 2]), ids(&[2, 1]));

    let g = graph(&[soft]);
    assert_eq!(up(&g, &[2], &[]).unwrap(), ids(&[2]));
    let g = graph(&[service(1, REF, 0x020000), soft]);
    assert_eq!(up(&g, &[2], &[]).unwrap(), ids(&[2]));
}
