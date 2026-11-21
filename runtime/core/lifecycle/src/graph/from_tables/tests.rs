extern crate std;

use super::*;
use crate::LiveSet;
use dkneeds::keys::{name_hash, write_table, Need as KNeed, Pair, Table};
use std::vec;
use std::vec::Vec;

const V1_2: u32 = 0x010200;
const V1_3: u32 = 0x010300;
const V2: u32 = 0x020000;

fn id(n: u8) -> PackageId {
    PackageId(n)
}

fn kneed(name: &str, min: u32, hard: bool) -> KNeed {
    KNeed {
        service: name_hash(name),
        min_version: min,
        hard,
    }
}

/// A header table with these needs and, for a service, its `PROVIDES`.
fn table(provides: &[(&str, u32)], needs: &[KNeed]) -> Vec<u8> {
    let provides: Vec<Pair> = provides
        .iter()
        .map(|(n, v)| Pair {
            a: name_hash(n),
            b: *v,
        })
        .collect();
    let t = Table {
        needs,
        provides: &provides,
        ..Table::default()
    };
    let mut out = vec![0u8; 512];
    let n = write_table(&t, &mut out).unwrap();
    out.truncate(n);
    out
}

fn graph(tables: &[(u8, Vec<u8>)], core: &[Api]) -> Result<PackageGraph, Refusal> {
    let v: Vec<(PackageId, &[u8])> = tables.iter().map(|(n, t)| (id(*n), t.as_slice())).collect();
    PackageGraph::from_tables(&v, core)
}

fn up(g: &PackageGraph, roots: &[u8], live: &[u8]) -> Result<Vec<PackageId>, Refusal> {
    let roots: Vec<PackageId> = roots.iter().map(|&n| id(n)).collect();
    let live: Vec<PackageId> = live.iter().map(|&n| id(n)).collect();
    g.bring_up_order(&roots, &LiveSet::of(&live))
        .map(|s| s.into_iter().collect())
}

#[test]
fn provides_and_hard_needs_become_edges() {
    let g = graph(
        &[
            (1, table(&[("ping", V1_2)], &[])),
            (2, table(&[], &[kneed("ping", V1_2, true)])),
        ],
        &[],
    )
    .unwrap();
    assert_eq!(
        g.node(id(1)).unwrap().provides(),
        Some(Api::new(name_hash("ping"), V1_2))
    );
    assert_eq!(up(&g, &[2], &[]).unwrap(), vec![id(1), id(2)]);
}

#[test]
fn a_need_on_a_core_api_makes_no_edge() {
    let core = [Api::new(name_hash("runtime"), V1_3)];
    let g = graph(&[(1, table(&[], &[kneed("runtime", V1_2, true)]))], &core).unwrap();
    assert_eq!(g.node(id(1)).unwrap().needs().len(), 0);
    assert_eq!(up(&g, &[1], &[]).unwrap(), vec![id(1)]);
}

#[test]
fn a_hard_need_above_a_core_api_is_refused_naming_the_package() {
    let core = [Api::new(name_hash("runtime"), V1_2)];
    let r = graph(&[(4, table(&[], &[kneed("runtime", V1_3, true)]))], &core);
    assert_eq!(
        r.unwrap_err(),
        Refusal::Incompatible {
            package: id(4),
            service: name_hash("runtime")
        }
    );
}

#[test]
fn a_table_that_does_not_parse_names_the_package() {
    let r = graph(&[(7, vec![0, 1, 2, 3])], &[]);
    assert_eq!(r.unwrap_err(), Refusal::BadTable { package: id(7) });
}

#[test]
fn two_provides_in_one_table_are_refused() {
    let r = graph(&[(3, table(&[("a", V1_2), ("b", V1_2)], &[]))], &[]);
    assert_eq!(r.unwrap_err(), Refusal::BadTable { package: id(3) });
}

#[test]
fn a_soft_need_orders_after_the_provider_and_never_refuses() {
    let soft = table(&[], &[kneed("ping", V1_2, false)]);
    let with = graph(
        &[(1, table(&[("ping", V1_2)], &[])), (2, soft.clone())],
        &[],
    )
    .unwrap();
    assert_eq!(up(&with, &[2], &[]).unwrap(), vec![id(1), id(2)]);
    // the provider has the higher id: the order still follows the need
    let late = graph(
        &[(2, table(&[("ping", V1_2)], &[])), (1, soft.clone())],
        &[],
    )
    .unwrap();
    assert_eq!(up(&late, &[1], &[]).unwrap(), vec![id(2), id(1)]);

    let absent = graph(&[(2, soft.clone())], &[]).unwrap();
    assert_eq!(up(&absent, &[2], &[]).unwrap(), vec![id(2)]);
    let old = graph(&[(1, table(&[("ping", V2)], &[])), (2, soft)], &[]).unwrap();
    assert_eq!(up(&old, &[2], &[]).unwrap(), vec![id(2)]);
}

#[test]
fn a_newer_minor_of_the_provider_meets_a_need_of_an_older_minor() {
    let g = graph(
        &[
            (1, table(&[("ping", V1_3)], &[])),
            (2, table(&[], &[kneed("ping", V1_2, true)])),
        ],
        &[],
    )
    .unwrap();
    assert_eq!(up(&g, &[2], &[]).unwrap(), vec![id(1), id(2)]);
}
