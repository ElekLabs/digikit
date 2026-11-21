//! The seam between the real packed header tables and the package graph (`PackageGraph::from_tables`): the `local-ping` fixtures as `patchkit pack` leaves
//! them in `dist/modules/<device>/`, read through the shared `dkneeds` parser. One case that fits (the orders of a shared service) and the refusal that is
//! typical for the seam (a consumer of a major version the provider lacks). The edge cases of the graph are unit tests in `dkrt-lifecycle`.
//!
//! The tests need the outputs of the Nx target `core/runtime:test-assembly` (its `dependsOn`). A device without its firmware is skipped, as in the other
//! assembly tests.

use crate::loader::tests::assembly_tests::{packed, table_of, DEVICES};
use dkneeds::keys::name_hash;
use dkrt_lifecycle::{Api, LiveSet, PackageGraph, PackageId, Refusal};
use std::vec::Vec;

const SERVICE: u8 = 1;
const USER: u8 = 2;
const USER_B: u8 = 3;
const OLD_USER: u8 = 4;

fn id(n: u8) -> PackageId {
    PackageId(n)
}

fn ids(v: &[u8]) -> Vec<PackageId> {
    v.iter().map(|&n| id(n)).collect()
}

fn live(v: &[u8]) -> LiveSet {
    LiveSet::of(&ids(v))
}

/// What the core provides without a package: the runtime floor the packed tables need.
fn core_apis() -> [Api; 1] {
    [Api::new(name_hash("runtime"), dkrt_sdk::RUNTIME_VERSION)]
}

fn graph(tables: &[(u8, &[u8])]) -> Result<PackageGraph, Refusal> {
    let v: Vec<(PackageId, &[u8])> = tables.iter().map(|(n, t)| (id(*n), *t)).collect();
    PackageGraph::from_tables(&v, &core_apis())
}

fn up(g: &PackageGraph, roots: &[u8], l: &[u8]) -> Result<Vec<PackageId>, Refusal> {
    g.bring_up_order(&ids(roots), &live(l))
        .map(|s| s.into_iter().collect())
}

fn down(g: &PackageGraph, target: u8, l: &[u8]) -> Vec<PackageId> {
    g.take_down_order(id(target), &live(l)).into_iter().collect()
}

#[test]
fn the_packed_local_ping_fixtures_give_the_orders_of_a_shared_service() {
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let tables = |leaf: &str, ext: &str| table_of(&packed(*dev, leaf, ext, &std::format!("test/{leaf}")));
        let service = tables("local-ping", "dks");
        let user = tables("local-ping-user", "dkm");
        let user_b = tables("local-ping-user-b", "dkm");
        let old_user = tables("local-ping-old-user", "dkm");

        let g = graph(&[(SERVICE, &service), (USER, &user), (USER_B, &user_b)]).unwrap();
        assert_eq!(up(&g, &[USER, USER_B], &[]).unwrap(), ids(&[SERVICE, USER, USER_B]), "{}: the service comes up before both users", dev.name);
        assert_eq!(down(&g, USER, &[SERVICE, USER, USER_B]), ids(&[USER]), "{}: user A goes, the service stays for B", dev.name);
        assert_eq!(down(&g, USER_B, &[SERVICE, USER_B]), ids(&[USER_B, SERVICE]), "{}: the last user takes the orphaned service down", dev.name);

        let g = graph(&[(SERVICE, &service), (OLD_USER, &old_user)]).unwrap();
        assert_eq!(
            up(&g, &[OLD_USER], &[]),
            Err(Refusal::Incompatible { package: id(OLD_USER), service: name_hash("local-ping") }),
            "{}: a need on major 2 of a service at major 1",
            dev.name
        );
    }
}
