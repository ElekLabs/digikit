extern crate std;

use super::*;
use crate::LiveSet;
use dkneeds::keys::{name_hash, write_table, KeyGroup, KindDecl, Need as KNeed, Pair, Table};
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

fn table_of(t: &Table) -> Vec<u8> {
    let mut out = vec![0u8; 512];
    let n = write_table(t, &mut out).unwrap();
    out.truncate(n);
    out
}

/// A table that has keys for kind `emits` and declares kind `owns` (0: none); a service when `provides` names one.
fn kind_table(provides: &[(&str, u32)], owns: u32, emits: u32) -> Vec<u8> {
    let provides: Vec<Pair> = provides
        .iter()
        .map(|(n, v)| Pair {
            a: name_hash(n),
            b: *v,
        })
        .collect();
    let kinds = [KindDecl {
        kind: owns,
        len: 16,
        entry: 8,
    }];
    let keys = [KeyGroup {
        kind: emits,
        keys: &[5],
    }];
    table_of(&Table {
        provides: &provides,
        kinds: if owns != 0 { &kinds } else { &[] },
        keys: if emits != 0 { &keys } else { &[] },
        ..Table::default()
    })
}

fn graph(tables: &[(u8, Vec<u8>)], core: &[Api]) -> Result<PackageGraph, Refusal> {
    let v: Vec<(PackageId, &[u8])> = tables.iter().map(|(n, t)| (id(*n), t.as_slice())).collect();
    PackageGraph::from_tables(&v, core)
}

fn down(g: &PackageGraph, target: u8, live: &[u8]) -> Vec<PackageId> {
    let live: Vec<PackageId> = live.iter().map(|&n| id(n)).collect();
    g.take_down_order(id(target), &LiveSet::of(&live))
        .into_iter()
        .collect()
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
fn a_hard_need_above_a_core_api_refuses_that_package_alone() {
    let core = [Api::new(name_hash("runtime"), V1_2)];
    let g = graph(
        &[
            (4, table(&[], &[kneed("runtime", V1_3, true)])),
            (5, table(&[], &[kneed("runtime", V1_2, true)])),
        ],
        &core,
    )
    .unwrap();
    assert_eq!(
        up(&g, &[4], &[]).unwrap_err(),
        Refusal::Incompatible {
            package: id(4),
            service: name_hash("runtime")
        }
    );
    assert_eq!(up(&g, &[5], &[]).unwrap(), vec![id(5)]);
}

#[test]
fn services_that_need_each_other_are_refused_alone_and_their_consumers_find_the_service_unmet() {
    let a = name_hash("a");
    let g = graph(
        &[
            (1, table(&[("a", 0)], &[kneed("b", 0, true)])),
            (2, table(&[("b", 0)], &[kneed("a", 0, true)])),
            (3, table(&[], &[kneed("a", 0, true)])),
            (4, table(&[("c", 0)], &[])),
            (5, table(&[], &[kneed("c", 0, true)])),
        ],
        &[],
    )
    .unwrap();
    // every package of the cycle is refused (no one of them is left to provide for the other)
    assert_eq!(
        up(&g, &[1], &[]).unwrap_err(),
        Refusal::Cycle { package: id(1) }
    );
    assert_eq!(
        up(&g, &[2], &[]).unwrap_err(),
        Refusal::Cycle { package: id(2) }
    );
    assert_eq!(
        up(&g, &[3], &[]).unwrap_err(),
        Refusal::Unmet {
            package: id(3),
            service: a
        }
    );
    // what is not near the cycle comes up
    assert_eq!(up(&g, &[5], &[]).unwrap(), vec![id(4), id(5)]);
}

#[test]
fn a_second_service_of_one_name_and_major_is_refused_alone() {
    let g = graph(
        &[
            (1, table(&[("s", V1_2)], &[])),
            (2, table(&[("s", V1_3)], &[])),
            (3, table(&[], &[kneed("s", V1_2, true)])),
        ],
        &[],
    )
    .unwrap();
    assert!(matches!(
        up(&g, &[2], &[]).unwrap_err(),
        Refusal::DuplicateService { package, .. } if package == id(2)
    ));
    assert_eq!(up(&g, &[3], &[]).unwrap(), vec![id(1), id(3)]);
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
    // a provider older than the need is no edge
    let old = graph(
        &[
            (1, table(&[("ping", V1_2)], &[])),
            (2, table(&[], &[kneed("ping", V1_3, false)])),
        ],
        &[],
    )
    .unwrap();
    assert_eq!(up(&old, &[2], &[]).unwrap(), vec![id(2)]);
    // another major is one when the consumer imports the service's own functions
    let imports = [Pair {
        a: name_hash("dk_ping_call"),
        b: name_hash("ping"),
    }];
    let t = Table {
        needs: &[kneed("ping", V1_2, false)],
        service_imports: &imports,
        ..Table::default()
    };
    let major = graph(&[(1, table(&[("ping", V2)], &[])), (2, table_of(&t))], &[]).unwrap();
    assert_eq!(up(&major, &[2], &[]).unwrap(), vec![id(2)]);
}

#[test]
fn a_need_of_zero_is_met_by_any_version_of_the_service() {
    // the old planner: for a provider a minimum of 0 is any version, with or without imports of the service's functions
    let imports = [Pair {
        a: name_hash("dk_ping_call"),
        b: name_hash("ping"),
    }];
    let checked = table_of(&Table {
        needs: &[kneed("ping", 0, true)],
        service_imports: &imports,
        ..Table::default()
    });
    let plain = table(&[], &[kneed("ping", 0, true)]);
    for provider in [V1_2, V2] {
        let g = graph(
            &[
                (1, table(&[("ping", provider)], &[])),
                (2, plain.clone()),
                (3, checked.clone()),
            ],
            &[],
        )
        .unwrap();
        assert_eq!(
            up(&g, &[2, 3], &[]).unwrap(),
            vec![id(1), id(2), id(3)],
            "a service at {provider:#x}"
        );
    }
}

#[test]
fn a_need_below_another_major_is_met_unless_the_consumer_imports_the_services_functions() {
    // a legacy need carries a runtime version, not the service's: a provider at 1.x meets a need of 0.1 (the old rule for a need with no `SERVICE_IMPORTS` item)
    let legacy = table(&[], &[kneed("ping", 0x000100, true)]);
    let imports = [Pair {
        a: name_hash("dk_ping_call"),
        b: name_hash("ping"),
    }];
    let own = table_of(&Table {
        needs: &[kneed("ping", 0x000100, true)],
        service_imports: &imports,
        ..Table::default()
    });
    let g = graph(
        &[(1, table(&[("ping", V1_2)], &[])), (2, legacy), (3, own)],
        &[],
    )
    .unwrap();
    assert_eq!(up(&g, &[2], &[]).unwrap(), vec![id(1), id(2)]);
    // the import names this service: the major version counts, and 0.1 is not 1.x
    assert_eq!(
        up(&g, &[3], &[]).unwrap_err(),
        Refusal::Incompatible {
            package: id(3),
            service: name_hash("ping")
        }
    );
    // an import of another service does not make this need checked
    let elsewhere = table_of(&Table {
        needs: &[kneed("ping", 0x000100, true)],
        service_imports: &[Pair {
            a: 1,
            b: name_hash("pong"),
        }],
        ..Table::default()
    });
    let g = graph(&[(1, table(&[("ping", V1_2)], &[])), (4, elsewhere)], &[]).unwrap();
    assert_eq!(up(&g, &[4], &[]).unwrap(), vec![id(1), id(4)]);
}

#[test]
fn a_core_api_of_a_newer_runtime_than_the_need_meets_it_whatever_the_major() {
    // only a floor newer than the runtime refuses: a need of 0 and one of an older version are met, also by a core of another major
    let core = [Api::new(name_hash("runtime"), V2)];
    let g = graph(
        &[
            (1, table(&[], &[kneed("runtime", 0, true)])),
            (2, table(&[], &[kneed("runtime", 0x000900, true)])),
            (3, table(&[], &[kneed("runtime", 0x020001, true)])),
            (4, table(&[], &[kneed("runtime", 0x020001, false)])),
        ],
        &core,
    )
    .unwrap();
    assert_eq!(up(&g, &[1, 2, 4], &[]).unwrap(), vec![id(1), id(2), id(4)]);
    assert_eq!(
        up(&g, &[3], &[]).unwrap_err(),
        Refusal::Incompatible {
            package: id(3),
            service: name_hash("runtime")
        }
    );
}

#[test]
fn a_package_with_keys_for_a_kind_comes_up_after_the_package_that_declares_it() {
    let g = graph(
        &[
            (1, kind_table(&[], 0, 90)),
            (2, kind_table(&[], 90, 0)),
            (3, kind_table(&[], 0, 91)),
        ],
        &[],
    )
    .unwrap();
    assert_eq!(g.node(id(1)).unwrap().kind_owners(), [id(2)]);
    assert_eq!(up(&g, &[1], &[]).unwrap(), vec![id(2), id(1)]);
    assert_eq!(g.consumers_of(id(2)).collect::<Vec<_>>(), vec![id(1)]);
    // a kind nobody declares is no edge: the rules of the core refuse its keys
    assert_eq!(g.node(id(3)).unwrap().kind_owners(), []);
    assert_eq!(up(&g, &[3], &[]).unwrap(), vec![id(3)]);
    // the owner goes down with its last package that has keys for the kind, and not before
    assert_eq!(down(&g, 1, &[2, 1]), vec![id(1), id(2)]);
    assert_eq!(
        down(&g, 2, &[2, 1]),
        vec![id(1), id(2)],
        "the package with keys goes down first"
    );
}

#[test]
fn a_service_waits_only_for_services_and_a_package_never_for_itself() {
    let g = graph(
        &[
            // service 1 has keys for kind 90, which module 2 declares: no edge, all services come first
            (1, kind_table(&[("svc", 0)], 0, 90)),
            (2, kind_table(&[], 90, 0)),
            // service 3 declares kind 91 and has keys for it
            (3, kind_table(&[("own", 0)], 91, 91)),
            // service 4 has keys for kind 91 of service 3
            (4, kind_table(&[("emit", 0)], 0, 91)),
        ],
        &[],
    )
    .unwrap();
    assert_eq!(g.node(id(1)).unwrap().kind_owners(), []);
    assert_eq!(g.node(id(3)).unwrap().kind_owners(), []);
    assert_eq!(g.node(id(4)).unwrap().kind_owners(), [id(3)]);
}

#[test]
fn packages_that_declare_the_kinds_the_other_has_keys_for_are_both_refused() {
    // A declares 90 and has keys for 91, B declares 91 and has keys for 90: neither can come up first, and neither takes the other's kind
    let g = graph(
        &[
            (1, kind_table(&[], 90, 91)),
            (2, kind_table(&[], 91, 90)),
            (3, kind_table(&[], 0, 90)),
        ],
        &[],
    )
    .unwrap();
    assert_eq!(
        up(&g, &[1], &[]).unwrap_err(),
        Refusal::Cycle { package: id(1) }
    );
    assert_eq!(
        up(&g, &[2], &[]).unwrap_err(),
        Refusal::Cycle { package: id(2) }
    );
    // a package with keys for a kind of the one that cannot come up is not refused by the graph: the kind has no owner for the rules of the core to find
    assert_eq!(up(&g, &[3], &[]).unwrap(), vec![id(3)]);
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

#[test]
fn the_exports_of_a_module_do_not_meet_a_need() {
    // only a service's `PROVIDES` is an API: a module that exports a function under the name does not provide the service
    let exporter = {
        let t = Table {
            exports: &[Pair {
                a: name_hash("lfo_bus"),
                b: 0x40,
            }],
            ..Table::default()
        };
        let mut out = vec![0u8; 512];
        let n = write_table(&t, &mut out).unwrap();
        out.truncate(n);
        out
    };
    let g = graph(
        &[(1, exporter), (2, table(&[], &[kneed("lfo_bus", 0, true)]))],
        &[],
    )
    .unwrap();
    assert_eq!(
        up(&g, &[2], &[]).unwrap_err(),
        Refusal::Unmet {
            package: id(2),
            service: name_hash("lfo_bus")
        }
    );
}
