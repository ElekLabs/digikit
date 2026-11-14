//! Raw counters named through the address map: per-function and per-module self aggregates.
//!
//! A raw counter is a count at one address (a sampled PC, an exit PC), so a function's number is the count of the addresses inside it and
//! nothing else: callers are not credited with their callees, and nothing here adds inclusive costs. Series (`pc`, `idle_pc`, ...) and CPUs are
//! reported apart.

use crate::profile::{Metric, Profile};
use dkaddrmap::{AddrMap, Cpu, Layer, LoadTable, Resolver};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt::Write;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Func {
    /// The function name without the offset; an address no layer holds is the hex address.
    pub name: String,
    /// The module (the name the runtime reports) the function belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    /// `our`, `anchor`, `fw`; None when unnamed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer: Option<Layer>,
    pub self_count: u64,
    /// Of the total of this series and CPU, 0 to 100.
    pub share: f64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Owner {
    /// A module id, `runtime` (our code outside modules), `firmware` (anchors and firmware names) or `unnamed`.
    pub owner: String,
    pub self_count: u64,
    pub share: f64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SeriesReport {
    pub series: String,
    pub cpu: Cpu,
    /// The sum of all counters of the series on this CPU.
    pub total: u64,
    /// Counters (distinct addresses) behind the totals.
    pub rows: usize,
    pub functions: Vec<Func>,
    pub owners: Vec<Owner>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Report {
    pub run: String,
    pub target: String,
    pub clock: String,
    pub unit: String,
    pub metrics: Vec<Metric>,
    pub series: Vec<SeriesReport>,
    pub loss: Vec<String>,
    /// Modules named by the maps but missing from the load table: their names are missing from the report.
    pub unloaded: Vec<String>,
}

fn pct(n: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        100.0 * n as f64 / total as f64
    }
}

/// Aggregate the profile. `maps` and `loads` name the addresses; `loads` of the call is added to the load table the run carries. `top` keeps
/// the heaviest `top` functions of each series (the totals still count all).
pub fn build(
    p: &Profile,
    maps: &[AddrMap],
    loads: &LoadTable,
    top: usize,
) -> Result<Report, String> {
    let mut table = p.run.loads.clone().unwrap_or_default();
    table.modules.extend(loads.modules.iter().cloned());
    let resolver = Resolver::new(maps, &table)?;
    type Key = (String, Cpu);
    #[derive(Default)]
    struct Acc {
        rows: usize,
        total: u64,
        funcs: BTreeMap<String, Func>,
    }
    let mut acc: BTreeMap<Key, Acc> = BTreeMap::new();
    for r in &p.raw {
        let hit = resolver.lookup(r.cpu, r.addr);
        let a = acc.entry((r.series.clone(), r.cpu)).or_default();
        a.rows += 1;
        a.total += r.count;
        let (name, module, layer) = match hit {
            Some(h) => (h.name, h.module, Some(h.layer)),
            None => (format!("{:#x}", r.addr), None, None),
        };
        let f = a.funcs.entry(name.clone()).or_insert(Func {
            name,
            module,
            layer,
            self_count: 0,
            share: 0.0,
        });
        f.self_count += r.count;
    }
    let mut series = vec![];
    for ((name, cpu), a) in acc {
        let mut owners: BTreeMap<String, u64> = BTreeMap::new();
        let mut functions: Vec<Func> = a.funcs.into_values().collect();
        for f in &mut functions {
            f.share = pct(f.self_count, a.total);
            let owner = match (&f.module, f.layer) {
                (Some(m), _) => m.clone(),
                (None, Some(Layer::Our)) => "runtime".into(),
                (None, Some(_)) => "firmware".into(),
                (None, None) => "unnamed".into(),
            };
            *owners.entry(owner).or_default() += f.self_count;
        }
        functions.sort_by(|x, y| {
            y.self_count
                .cmp(&x.self_count)
                .then_with(|| x.name.cmp(&y.name))
        });
        functions.truncate(top);
        let mut owners: Vec<Owner> = owners
            .into_iter()
            .map(|(owner, self_count)| Owner {
                owner,
                self_count,
                share: pct(self_count, a.total),
            })
            .collect();
        owners.sort_by(|x, y| {
            y.self_count
                .cmp(&x.self_count)
                .then_with(|| x.owner.cmp(&y.owner))
        });
        series.push(SeriesReport {
            series: name,
            cpu,
            total: a.total,
            rows: a.rows,
            functions,
            owners,
        });
    }
    Ok(Report {
        run: p.run.id.clone(),
        target: p.run.target.clone(),
        clock: p.clock.source.clone(),
        unit: p.clock.unit.clone(),
        metrics: p.metrics.clone(),
        series,
        loss: p
            .loss
            .iter()
            .map(|l| match l.dropped {
                Some(n) => format!("{}: {} ({n} dropped)", l.series, l.reason),
                None => format!("{}: {}", l.series, l.reason),
            })
            .collect(),
        unloaded: resolver.unloaded.clone(),
    })
}

fn metric_text(m: &Metric) -> String {
    match (m.value, &m.na_reason) {
        (Some(v), _) if v.fract() == 0.0 && v.abs() < 1e15 => format!("{} {}", v as i64, m.unit),
        (Some(v), _) => format!("{v:.3} {}", m.unit),
        (None, Some(r)) => format!("N/A ({r})"),
        (None, None) => "N/A".into(),
    }
}

/// The text view of a report.
pub fn text(r: &Report, top: usize) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "run {} target {} clock {} ({})",
        r.run, r.target, r.clock, r.unit
    );
    if !r.metrics.is_empty() {
        let _ = writeln!(s, "metrics:");
        for m in &r.metrics {
            let _ = writeln!(s, "  {:<34} {}", m.name, metric_text(m));
        }
    }
    for x in &r.series {
        let cpu = format!("{:?}", x.cpu).to_lowercase();
        let _ = writeln!(
            s,
            "{} {cpu} by function (top {top}; {} rows, total {}):",
            x.series, x.rows, x.total
        );
        for f in x.functions.iter().take(top) {
            let _ = writeln!(s, "  {:>14} {:5.1}%  {}", f.self_count, f.share, f.name);
        }
        let _ = writeln!(s, "{} {cpu} by owner:", x.series);
        for o in &x.owners {
            let _ = writeln!(s, "  {:>14} {:5.1}%  {}", o.self_count, o.share, o.owner);
        }
    }
    for l in &r.loss {
        let _ = writeln!(s, "loss: {l}");
    }
    if !r.unloaded.is_empty() {
        let _ = writeln!(
            s,
            "not in the load table (unnamed): {}",
            r.unloaded.join(", ")
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{Clock, Raw, Run, SCHEMA};
    use dkaddrmap::{Entry, Key, Load};

    fn entry(
        cpu: Cpu,
        addr: u64,
        size: u64,
        name: &str,
        layer: Layer,
        module: Option<&str>,
    ) -> Entry {
        Entry {
            cpu,
            addr,
            size,
            name: name.into(),
            layer,
            module: module.map(String::from),
        }
    }

    fn maps() -> Vec<AddrMap> {
        vec![
            AddrMap::new(
                "ship",
                Key::default(),
                vec![
                    entry(Cpu::Cf, 0x4000_0000, 0x100, "rt_main", Layer::Our, None),
                    entry(Cpu::Cf, 0x4000_0100, 0x40, "fw_hook", Layer::Anchor, None),
                ],
            ),
            AddrMap::new(
                "module",
                Key::default(),
                vec![
                    entry(Cpu::Cf, 0, 0x80, "init", Layer::Our, Some("digikit/pluck")),
                    entry(
                        Cpu::Cf,
                        0x80,
                        0x80,
                        "render",
                        Layer::Our,
                        Some("digikit/pluck"),
                    ),
                    entry(
                        Cpu::Sharc,
                        0x10,
                        0x20,
                        "_pk_render",
                        Layer::Our,
                        Some("digikit/pluck"),
                    ),
                ],
            ),
        ]
    }

    fn profile(raw: Vec<Raw>) -> Profile {
        Profile {
            schema: SCHEMA.into(),
            run: Run {
                id: "t".into(),
                target: "dn2".into(),
                loads: Some(LoadTable {
                    modules: vec![Load {
                        id: "digikit/pluck".into(),
                        base: 0x4700_0000,
                        size: 0x100,
                        dsp_base: Some(0xb8_1000),
                        fini: None,
                    }],
                }),
                ..Run::default()
            },
            clock: Clock {
                source: "emu.cf.insn".into(),
                unit: "insn".into(),
                sampling: "synthetic".into(),
            },
            raw,
            metrics: vec![],
            markers: vec![],
            loss: vec![],
            overhead: vec![],
        }
    }

    fn raw(cpu: Cpu, addr: u64, count: u64, series: &str) -> Raw {
        Raw {
            cpu,
            addr,
            count,
            series: series.into(),
        }
    }

    #[test]
    fn counters_become_self_aggregates_per_function_and_owner() {
        let p = profile(vec![
            raw(Cpu::Cf, 0x4700_0084, 30, "pc"),
            raw(Cpu::Cf, 0x4700_0090, 10, "pc"), // the same function: summed
            raw(Cpu::Cf, 0x4000_0010, 50, "pc"),
            raw(Cpu::Cf, 0x4000_0104, 5, "pc"),
            raw(Cpu::Cf, 0x1234_5678, 5, "pc"), // no layer holds it
        ]);
        let r = build(&p, &maps(), &LoadTable::default(), 10).unwrap();
        assert_eq!(r.series.len(), 1);
        let s = &r.series[0];
        assert_eq!(s.total, 100);
        let names: Vec<(&str, u64)> = s
            .functions
            .iter()
            .map(|f| (f.name.as_str(), f.self_count))
            .collect();
        assert_eq!(
            names,
            [
                ("rt_main", 50),
                ("render", 40),
                ("0x12345678", 5),
                ("fw_hook", 5)
            ]
        );
        let owners: Vec<(&str, u64)> = s
            .owners
            .iter()
            .map(|o| (o.owner.as_str(), o.self_count))
            .collect();
        assert_eq!(
            owners,
            [
                ("runtime", 50),
                ("digikit/pluck", 40),
                ("firmware", 5),
                ("unnamed", 5)
            ]
        );
        assert_eq!(s.owners.iter().map(|o| o.self_count).sum::<u64>(), s.total);
    }

    #[test]
    fn series_and_cpus_stay_apart_and_dsp_names_use_dsp_base() {
        let p = profile(vec![
            raw(Cpu::Cf, 0x4700_0084, 7, "pc"),
            raw(Cpu::Cf, 0x4700_0084, 3, "idle_pc"),
            raw(Cpu::Sharc, 0xb8_1018, 9, "pc"),
        ]);
        let r = build(&p, &maps(), &LoadTable::default(), 10).unwrap();
        let keys: Vec<(&str, Cpu, u64)> = r
            .series
            .iter()
            .map(|s| (s.series.as_str(), s.cpu, s.total))
            .collect();
        assert_eq!(
            keys,
            [
                ("idle_pc", Cpu::Cf, 3),
                ("pc", Cpu::Cf, 7),
                ("pc", Cpu::Sharc, 9)
            ]
        );
        assert_eq!(r.series[2].functions[0].name, "_pk_render");
    }

    #[test]
    fn top_cuts_the_list_but_not_the_totals() {
        let p = profile(
            (0..5)
                .map(|i| raw(Cpu::Cf, 0x2000_0000 + i, 1, "pc"))
                .collect(),
        );
        let r = build(&p, &maps(), &LoadTable::default(), 2).unwrap();
        assert_eq!(r.series[0].functions.len(), 2);
        assert_eq!(r.series[0].total, 5);
        assert_eq!(r.series[0].rows, 5);
    }

    #[test]
    fn a_module_missing_from_the_load_table_is_reported() {
        let mut p = profile(vec![raw(Cpu::Cf, 0x4700_0084, 1, "pc")]);
        p.run.loads = None;
        let r = build(&p, &maps(), &LoadTable::default(), 10).unwrap();
        assert_eq!(r.unloaded, ["digikit/pluck"]);
        assert_eq!(r.series[0].functions[0].name, "0x47000084");
    }

    #[test]
    fn na_metrics_keep_their_reason_and_a_bare_metric_is_refused() {
        let mut p = profile(vec![]);
        p.metrics = vec![
            Metric::value("path.run_fast.instructions", 1234.0, "insn"),
            Metric::na("host.cpu", "ns", "the device has no host CPU"),
        ];
        let text = crate::report::text(&build(&p, &[], &LoadTable::default(), 5).unwrap(), 5);
        assert!(text.contains("1234 insn"), "{text}");
        assert!(text.contains("N/A (the device has no host CPU)"), "{text}");
        let bad = r#"{"schema":"dk.profile/1","run":{"id":"x","target":"dn2"},"clock":{"source":"s","unit":"u","sampling":""},
            "raw":[],"metrics":[{"name":"m","unit":"ns"}]}"#;
        assert!(Profile::parse(bad).unwrap_err().contains("never a zero"));
    }

    #[test]
    fn a_profile_round_trips_and_checks_its_schema() {
        let p = profile(vec![raw(Cpu::Cf, 1, 2, "pc")]);
        assert_eq!(Profile::parse(&p.to_json()).unwrap(), p);
        let other = p.to_json().replace("dk.profile/1", "dk.profile/2");
        assert!(Profile::parse(&other).unwrap_err().contains("schema"));
    }
}
