//! Device calibration (docs/plans/profiling.md 13.2): the factor between what a device measured and what the model (the emulator) counted.
//!
//! Input: device profiles and model profiles (`dk.profile/1` with `dsp_rows` and `dsp` from the DSP stats window, and `spans`), several per case
//! and several cases. A case is named by `run.case_id`; a device capture and a model capture of one case must agree on target, firmware, module
//! hashes and `run.input_sha256`, and every row (DSP) or owner (ColdFire) must have equal calls per block (DSP) or equal calls (spans). Anything else is
//! refused with a reason and listed in `refused`; it never drops out silently.
//!
//! The DSP side fits one factor `f` with `cycles ~ f * instructions`, from the worst-case statistics (largest call, largest block), not the means:
//! `fit` minimises the sum of squared relative errors, `budget` is the worst observed cycles/instructions ratio, so it never underpredicts a
//! measured pair. Steady means are paired and reported separately. The ColdFire side has the same shape over span owners, in time (ticks over the
//! span clock). The parsing is tolerant: a profile without `dsp_rows` or `spans` simply gives no pairs for that side.
//!
//! Result: [`Calibration`], the file `dk.calibration/1` (docs/INSTRUMENTATION.md "Calibration").

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write as _;

pub const SCHEMA: &str = "dk.calibration/1";
/// A module-row spread above this (relative, worst ratio over best) says that one factor is too coarse.
pub const SPREAD_LIMIT: f64 = 0.10;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Request {
    /// Device profiles (`dk.profile/1`), several captures per case.
    pub device: Vec<Value>,
    /// Model profiles (the emulator), same cases.
    pub model: Vec<Value>,
    /// Measured DSP block cycles of the device (finding 103), for the percentage of a block.
    #[serde(default)]
    pub block_cycles: Option<u64>,
    /// The instruction-count model: daemon sha256, CORE_FINGERPRINT or rig core fingerprint.
    #[serde(default)]
    pub model_id: Option<String>,
    /// Names and hashes of the files the profiles came from, kept in the output as given.
    #[serde(default)]
    pub sources: Option<Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Key {
    pub device: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub firmware_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_cycles: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// `cycles` for a device capture; `insn` means both sides were emulators (a self-check, not a calibration).
    pub device_unit: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Factor {
    /// Least squares on the relative error.
    pub fit: f64,
    /// The worst observed ratio: never below a measured pair.
    pub budget: f64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct ErrorStats {
    pub n: usize,
    /// Mean of `(f*model - device)/device` over the worst-case pairs, with the fit factor.
    pub mean: f64,
    /// The worst underprediction of the fit factor, as a positive fraction (0 when it never underpredicts).
    pub worst_under: f64,
    /// The worst underprediction of a row (or owner) of a case predicted with a fit that left it out.
    pub loo_worst: f64,
    /// The same with the budget factor of the others: the budget never underpredicting a row it has not seen.
    pub loo_budget_under: f64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Steady {
    pub n: usize,
    pub mean: f64,
    pub worst_under: f64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Stock {
    /// Per case: the largest stock block delta of the device minus the largest blocks of the module rows.
    pub avg: f64,
    pub max: f64,
    pub cases: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Pair {
    pub case: String,
    /// `row 3` (DSP) or `module 0x1`/`phase 0x101` (ColdFire owner).
    pub id: String,
    /// `max_call`, `max_block` (DSP), `max` (spans) or `mean`.
    pub stat: String,
    /// Model value: instructions (DSP) or microseconds (spans).
    pub model: f64,
    /// Device value: cycles (DSP) or microseconds (spans).
    pub device: f64,
    pub ratio: f64,
    /// `(fit * model - device) / device`.
    pub error: f64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Section {
    pub factor: Factor,
    pub error: ErrorStats,
    pub steady: Steady,
    /// `(worst ratio - best ratio) / best ratio` over the worst-case pairs.
    pub spread: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stock: Option<Stock>,
    pub pairs: Vec<Pair>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Refused {
    /// `dsp`, `cf` or `case`.
    pub section: String,
    pub case: String,
    pub id: String,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Calibration {
    pub schema: String,
    pub key: Key,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dsp: Option<Section>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cf: Option<Section>,
    pub refused: Vec<Refused>,
    pub warnings: Vec<String>,
}

// ---- reading a profile (tolerant) ----

#[derive(Clone, Debug, Default)]
struct DspStat {
    calls: u64,
    sum: u64,
    max_call: u64,
    max_block: u64,
}

#[derive(Clone, Debug, Default)]
struct SpanStat {
    kind: String,
    calls: u64,
    sum: u64,
    max: u64,
}

#[derive(Clone, Debug, Default)]
struct Side {
    target: String,
    firmware: Option<String>,
    modules: Vec<String>,
    input: Option<String>,
    unit: Option<String>,
    blocks: u64,
    stock_delta: Option<u64>,
    rows: BTreeMap<u64, DspStat>,
    spans: BTreeMap<u64, SpanStat>,
    hz: Option<u64>,
}

fn u(v: &Value, k: &str) -> u64 {
    v[k].as_u64().unwrap_or(0)
}

fn read_side(p: &Value) -> Result<(Option<String>, Side), String> {
    if p["schema"] != "dk.profile/1" {
        return Err(format!(
            "profile schema {:?}, expected dk.profile/1",
            p["schema"]
        ));
    }
    let run = &p["run"];
    let mut modules: Vec<String> = run["module_sha256"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    modules.sort();
    let mut s = Side {
        target: run["target"].as_str().unwrap_or("").to_owned(),
        firmware: run["firmware_sha256"].as_str().map(str::to_owned),
        modules,
        input: run["input_sha256"].as_str().map(str::to_owned),
        hz: p["span_clock_hz"].as_u64(),
        ..Side::default()
    };
    if let Some(d) = p.get("dsp").filter(|d| d.is_object()) {
        s.unit = d["unit"].as_str().map(str::to_owned);
        s.blocks = u(d, "blocks");
        s.stock_delta = d["max_stock_block_delta"].as_u64();
    }
    for r in p["dsp_rows"].as_array().into_iter().flatten() {
        let row = r["row"].as_u64().ok_or("dsp row without a row number")?;
        let e = s.rows.entry(row).or_default();
        e.calls += u(r, "calls");
        e.sum += u(r, "sum_cycles");
        e.max_call = e.max_call.max(u(r, "max_call"));
        e.max_block = e.max_block.max(u(r, "max_block"));
    }
    for r in p["spans"].as_array().into_iter().flatten() {
        let owner = r["owner"].as_u64().ok_or("span without an owner")?;
        let e = s.spans.entry(owner).or_default();
        e.kind = r["kind"].as_str().unwrap_or("").to_owned();
        e.calls += u(r, "calls");
        e.sum += u(r, "sum_ticks");
        e.max = e.max.max(u(r, "max_ticks"));
    }
    Ok((run["case_id"].as_str().map(str::to_owned), s))
}

/// Several captures of one case on one side: counters add up, largest values stay largest.
fn merge(mut parts: Vec<Side>, who: &str, case: &str) -> Result<Side, String> {
    let mut it = parts.drain(..);
    let mut a = it.next().ok_or("empty case")?;
    for b in it {
        let same = a.target == b.target
            && a.firmware == b.firmware
            && a.modules == b.modules
            && a.input == b.input
            && a.unit == b.unit
            && a.hz == b.hz;
        if !same {
            return Err(format!(
                "{who} captures of case {case} differ in target, firmware, modules, input hash, unit or span clock"
            ));
        }
        a.blocks += b.blocks;
        a.stock_delta = match (a.stock_delta, b.stock_delta) {
            (Some(x), Some(y)) => Some(x.max(y)),
            (x, y) => x.or(y),
        };
        for (k, v) in b.rows {
            let e = a.rows.entry(k).or_default();
            e.calls += v.calls;
            e.sum += v.sum;
            e.max_call = e.max_call.max(v.max_call);
            e.max_block = e.max_block.max(v.max_block);
        }
        for (k, v) in b.spans {
            let e = a.spans.entry(k).or_default();
            e.kind = v.kind;
            e.calls += v.calls;
            e.sum += v.sum;
            e.max = e.max.max(v.max);
        }
    }
    Ok(a)
}

fn group(
    list: &[Value],
    who: &str,
    refused: &mut Vec<Refused>,
) -> Result<BTreeMap<String, Side>, String> {
    let mut by: BTreeMap<String, Vec<Side>> = BTreeMap::new();
    for (i, p) in list.iter().enumerate() {
        let (case, side) = read_side(p).map_err(|e| format!("{who} profile {i}: {e}"))?;
        let Some(case) = case else {
            return Err(format!(
                "{who} profile {i} has no run.case_id (a calibration pairs by case)"
            ));
        };
        by.entry(case).or_default().push(side);
    }
    let mut out = BTreeMap::new();
    for (case, parts) in by {
        match merge(parts, who, &case) {
            Ok(s) => {
                out.insert(case, s);
            }
            Err(reason) => refused.push(Refused {
                section: "case".into(),
                case,
                id: who.into(),
                reason,
            }),
        }
    }
    Ok(out)
}

// ---- the fit ----

struct Raw {
    case: String,
    id: String,
    stat: &'static str,
    model: f64,
    device: f64,
}

fn ratio(p: &Raw) -> f64 {
    p.device / p.model
}

/// `f` minimising the sum of `(f*x - 1)^2` with `x = model/device`.
fn fit_of(worst: &[&Raw]) -> f64 {
    let (mut sx, mut sxx) = (0.0, 0.0);
    for p in worst {
        let x = p.model / p.device;
        sx += x;
        sxx += x * x;
    }
    if sxx == 0.0 { 0.0 } else { sx / sxx }
}

fn budget_of(worst: &[&Raw]) -> f64 {
    worst.iter().map(|p| ratio(p)).fold(0.0, f64::max)
}

fn err(f: f64, p: &Raw) -> f64 {
    (f * p.model - p.device) / p.device
}

fn under(e: f64) -> f64 {
    (-e).max(0.0)
}

fn is_worst(p: &Raw) -> bool {
    p.stat != "mean"
}

/// Build a section from pairs: worst-case pairs fit; means are reported against the fit.
fn section(raw: Vec<Raw>) -> Option<Section> {
    let worst: Vec<&Raw> = raw.iter().filter(|p| is_worst(p)).collect();
    if worst.is_empty() {
        return None;
    }
    let fit = fit_of(&worst);
    let budget = budget_of(&worst);
    let errs: Vec<f64> = worst.iter().map(|p| err(fit, p)).collect();
    let n = errs.len();
    let mean = errs.iter().sum::<f64>() / n as f64;
    let worst_under = errs.iter().copied().map(under).fold(0.0, f64::max);
    // leave one (case, row/owner) out: refit on the rest, predict the left-out pairs
    let mut groups: Vec<(&str, &str)> = worst
        .iter()
        .map(|p| (p.case.as_str(), p.id.as_str()))
        .collect();
    groups.sort();
    groups.dedup();
    let (mut loo_worst, mut loo_budget_under) = (0.0f64, 0.0f64);
    if groups.len() > 1 {
        for g in &groups {
            let rest: Vec<&Raw> = worst
                .iter()
                .copied()
                .filter(|p| (p.case.as_str(), p.id.as_str()) != *g)
                .collect();
            let (f, b) = (fit_of(&rest), budget_of(&rest));
            for p in worst
                .iter()
                .filter(|p| (p.case.as_str(), p.id.as_str()) == *g)
            {
                loo_worst = loo_worst.max(under(err(f, p)));
                loo_budget_under = loo_budget_under.max(under(err(b, p)));
            }
        }
    }
    let means: Vec<f64> = raw
        .iter()
        .filter(|p| !is_worst(p))
        .map(|p| err(fit, p))
        .collect();
    let steady = Steady {
        n: means.len(),
        mean: if means.is_empty() {
            0.0
        } else {
            means.iter().sum::<f64>() / means.len() as f64
        },
        worst_under: means.iter().copied().map(under).fold(0.0, f64::max),
    };
    let (lo, hi) = worst
        .iter()
        .map(|p| ratio(p))
        .fold((f64::MAX, 0.0f64), |(l, h), r| (l.min(r), h.max(r)));
    let spread = if lo > 0.0 { (hi - lo) / lo } else { 0.0 };
    let pairs = raw
        .iter()
        .map(|p| Pair {
            case: p.case.clone(),
            id: p.id.clone(),
            stat: p.stat.into(),
            model: p.model,
            device: p.device,
            ratio: ratio(p),
            error: err(fit, p),
        })
        .collect();
    Some(Section {
        factor: Factor { fit, budget },
        error: ErrorStats {
            n,
            mean,
            worst_under,
            loo_worst,
            loo_budget_under,
        },
        steady,
        spread,
        stock: None,
        pairs,
    })
}

fn refuse(
    out: &mut Vec<Refused>,
    section: &str,
    case: &str,
    id: impl Into<String>,
    reason: impl Into<String>,
) {
    out.push(Refused {
        section: section.into(),
        case: case.into(),
        id: id.into(),
        reason: reason.into(),
    });
}

/// The case-level key both sides must share; `Some(reason)` when they do not.
fn key_mismatch(d: &Side, m: &Side) -> Option<String> {
    if d.target != m.target {
        return Some(format!(
            "device {:?} against model {:?}",
            d.target, m.target
        ));
    }
    if d.firmware.is_some() && m.firmware.is_some() && d.firmware != m.firmware {
        return Some("firmware sha256 differs".into());
    }
    if d.modules != m.modules {
        return Some("module sha256 lists differ".into());
    }
    if d.input != m.input {
        return Some("input hash differs (or one side has none)".into());
    }
    None
}

fn dsp_pairs(case: &str, d: &Side, m: &Side, raw: &mut Vec<Raw>, refused: &mut Vec<Refused>) {
    for (row, dr) in &d.rows {
        let id = format!("row {row}");
        if dr.calls == 0 && m.rows.get(row).is_none_or(|x| x.calls == 0) {
            continue;
        }
        let Some(mr) = m.rows.get(row) else {
            refuse(
                refused,
                "dsp",
                case,
                &id,
                "unpaired: the model has no such row",
            );
            continue;
        };
        if d.blocks == 0 || m.blocks == 0 {
            refuse(
                refused,
                "dsp",
                case,
                &id,
                "a side has no blocks in its header",
            );
            continue;
        }
        // equal calls per block: dc/db == mc/mb
        if u128::from(dr.calls) * u128::from(m.blocks)
            != u128::from(mr.calls) * u128::from(d.blocks)
        {
            refuse(
                refused,
                "dsp",
                case,
                &id,
                format!(
                    "call-count mismatch: device {} calls in {} blocks, model {} calls in {} blocks",
                    dr.calls, d.blocks, mr.calls, m.blocks
                ),
            );
            continue;
        }
        if dr.calls == 0 {
            continue;
        }
        let stats = [
            ("max_call", dr.max_call as f64, mr.max_call as f64),
            ("max_block", dr.max_block as f64, mr.max_block as f64),
            (
                "mean",
                dr.sum as f64 / dr.calls as f64,
                mr.sum as f64 / mr.calls as f64,
            ),
        ];
        for (stat, dv, mv) in stats {
            if dv == 0.0 && mv == 0.0 {
                continue;
            }
            if dv <= 0.0 || mv <= 0.0 {
                refuse(
                    refused,
                    "dsp",
                    case,
                    format!("{id} {stat}"),
                    "one side counted zero, the other did not",
                );
                continue;
            }
            raw.push(Raw {
                case: case.into(),
                id: id.clone(),
                stat,
                model: mv,
                device: dv,
            });
        }
    }
    for row in m.rows.keys() {
        if !d.rows.contains_key(row) && m.rows[row].calls > 0 {
            refuse(
                refused,
                "dsp",
                case,
                format!("row {row}"),
                "unpaired: the device has no such row",
            );
        }
    }
}

fn cf_pairs(case: &str, d: &Side, m: &Side, raw: &mut Vec<Raw>, refused: &mut Vec<Refused>) {
    let (Some(dhz), Some(mhz)) = (d.hz, m.hz) else {
        if !d.spans.is_empty() || !m.spans.is_empty() {
            refuse(refused, "cf", case, "*", "a side has no span_clock_hz");
        }
        return;
    };
    let us = |ticks: f64, hz: u64| ticks * 1e6 / hz as f64;
    for (owner, ds) in &d.spans {
        let id = format!(
            "{} {:#x}",
            if ds.kind.is_empty() {
                "owner"
            } else {
                &ds.kind
            },
            owner
        );
        let Some(ms) = m.spans.get(owner) else {
            refuse(
                refused,
                "cf",
                case,
                &id,
                "unpaired: the model has no such owner",
            );
            continue;
        };
        if ds.kind != ms.kind {
            refuse(
                refused,
                "cf",
                case,
                &id,
                format!("key mismatch: kind {:?} against {:?}", ds.kind, ms.kind),
            );
            continue;
        }
        if ds.calls != ms.calls {
            refuse(
                refused,
                "cf",
                case,
                &id,
                format!(
                    "call-count mismatch: device {} calls, model {}",
                    ds.calls, ms.calls
                ),
            );
            continue;
        }
        if ds.calls == 0 {
            continue;
        }
        let stats = [
            ("max", ds.max as f64, ms.max as f64),
            (
                "mean",
                ds.sum as f64 / ds.calls as f64,
                ms.sum as f64 / ms.calls as f64,
            ),
        ];
        for (stat, dt, mt) in stats {
            if dt == 0.0 && mt == 0.0 {
                continue;
            }
            if dt <= 0.0 || mt <= 0.0 {
                refuse(
                    refused,
                    "cf",
                    case,
                    format!("{id} {stat}"),
                    "one side counted zero ticks, the other did not",
                );
                continue;
            }
            raw.push(Raw {
                case: case.into(),
                id: id.clone(),
                stat,
                model: us(mt, mhz),
                device: us(dt, dhz),
            });
        }
    }
    for (owner, ms) in &m.spans {
        if !d.spans.contains_key(owner) && ms.calls > 0 {
            refuse(
                refused,
                "cf",
                case,
                format!("owner {owner:#x}"),
                "unpaired: the device has no such owner",
            );
        }
    }
}

/// Fit the calibration of a set of cases.
pub fn fit(req: &Request) -> Result<Calibration, String> {
    let mut refused = Vec::new();
    let dev = group(&req.device, "device", &mut refused)?;
    let mdl = group(&req.model, "model", &mut refused)?;
    if dev.is_empty() || mdl.is_empty() {
        return Err("calibrate: needs at least one device and one model profile".into());
    }
    let mut raw_dsp = Vec::new();
    let mut raw_cf = Vec::new();
    let mut stocks = Vec::new();
    let mut warnings = Vec::new();
    let (mut target, mut firmware, mut unit): (Option<String>, Option<String>, Option<String>) =
        (None, None, None);
    for (case, d) in &dev {
        let Some(m) = mdl.get(case) else {
            refuse(
                &mut refused,
                "case",
                case,
                "",
                "unpaired: no model profile for this case",
            );
            continue;
        };
        if let Some(reason) = key_mismatch(d, m) {
            refuse(
                &mut refused,
                "case",
                case,
                "",
                format!("key mismatch: {reason}"),
            );
            continue;
        }
        if let Some(t) = &target {
            if *t != d.target || firmware != d.firmware {
                return Err(format!(
                    "calibrate: case {case} was captured on {:?} firmware {:?}, the others on {t:?} {firmware:?}: one file holds one device and firmware",
                    d.target, d.firmware
                ));
            }
        } else {
            target = Some(d.target.clone());
            firmware = d.firmware.clone();
        }
        if let Some(mu) = m.unit.as_ref().filter(|mu| *mu != "insn") {
            refuse(
                &mut refused,
                "dsp",
                case,
                "",
                format!("the model unit is {mu:?}, expected insn"),
            );
            continue;
        }
        if d.unit.is_some() {
            if unit.is_some() && unit != d.unit {
                return Err("calibrate: device captures mix units (cycles and insn)".into());
            }
            unit = d.unit.clone();
        }
        dsp_pairs(case, d, m, &mut raw_dsp, &mut refused);
        cf_pairs(case, d, m, &mut raw_cf, &mut refused);
        match (d.stock_delta, d.unit.as_deref()) {
            (Some(delta), _) => {
                let rows: u64 = d.rows.values().map(|r| r.max_block).sum();
                stocks.push(delta.saturating_sub(rows) as f64);
            }
            (None, _) if !d.rows.is_empty() => {
                warnings.push(format!(
                    "case {case}: the device profile has no dsp header, no stock figure"
                ));
            }
            _ => {}
        }
    }
    let mut dsp = section(raw_dsp);
    let cf = section(raw_cf);
    if dsp.is_none() && cf.is_none() {
        let why: Vec<String> = refused
            .iter()
            .map(|r| format!("{} {} {}: {}", r.section, r.case, r.id, r.reason))
            .collect();
        return Err(format!(
            "calibrate: no pair to fit ({})",
            if why.is_empty() {
                "no data".into()
            } else {
                why.join("; ")
            }
        ));
    }
    if let (Some(s), false) = (dsp.as_mut(), stocks.is_empty()) {
        s.stock = Some(Stock {
            avg: stocks.iter().sum::<f64>() / stocks.len() as f64,
            max: stocks.iter().copied().fold(0.0, f64::max),
            cases: stocks.len(),
        });
    }
    for (name, s) in [("DSP", &dsp), ("ColdFire", &cf)] {
        if let Some(s) = s.as_ref().filter(|s| s.spread > SPREAD_LIMIT) {
            warnings.push(format!(
                "{name} spread {:.1}% is over {:.0}%: one factor per device is too coarse here (a per-class model is out of scope; the budget factor covers the worst row)",
                s.spread * 100.0,
                SPREAD_LIMIT * 100.0
            ));
        }
    }
    if unit.as_deref() == Some("insn") {
        warnings.push("the device side counted instructions: an emulator-against-emulator check, not a device calibration".into());
    }
    Ok(Calibration {
        schema: SCHEMA.into(),
        key: Key {
            device: target.unwrap_or_default(),
            firmware_sha256: firmware,
            block_cycles: req.block_cycles,
            model: req.model_id.clone(),
            device_unit: unit.unwrap_or_else(|| "cycles".into()),
        },
        sources: req.sources.clone(),
        dsp,
        cf,
        refused,
        warnings,
    })
}

// ---- views ----

fn pct(x: f64) -> String {
    format!("{:+.2}%", x * 100.0)
}

fn section_text(out: &mut String, name: &str, unit: &str, s: &Section) {
    let _ = writeln!(
        out,
        "{name}: factor fit {:.4} budget {:.4} ({unit})",
        s.factor.fit, s.factor.budget
    );
    let e = &s.error;
    let _ = writeln!(
        out,
        "  error (worst-case pairs): n {} mean {} worst under {:.2}% leave-one-out worst {:.2}% (budget {:.2}%)",
        e.n,
        pct(e.mean),
        e.worst_under * 100.0,
        e.loo_worst * 100.0,
        e.loo_budget_under * 100.0
    );
    if s.steady.n > 0 {
        let _ = writeln!(
            out,
            "  steady means: n {} mean {} worst under {:.2}%",
            s.steady.n,
            pct(s.steady.mean),
            s.steady.worst_under * 100.0
        );
    }
    let _ = writeln!(out, "  spread {:.1}%", s.spread * 100.0);
    if let Some(k) = &s.stock {
        let _ = writeln!(
            out,
            "  stock: avg {:.0} max {:.0} over {} cases (the budget uses max)",
            k.avg, k.max, k.cases
        );
    }
}

/// The text view of a calibration.
pub fn text(c: &Calibration) -> String {
    let mut o = String::new();
    let k = &c.key;
    let _ = writeln!(
        o,
        "# {} device {} firmware {} model {} block {} cycles",
        c.schema,
        k.device,
        k.firmware_sha256
            .as_deref()
            .map_or("?".into(), |h| h.chars().take(12).collect::<String>()),
        k.model
            .as_deref()
            .map_or("?".into(), |h| h.chars().take(12).collect::<String>()),
        k.block_cycles.map_or("?".into(), |b| b.to_string()),
    );
    if let Some(s) = &c.dsp {
        section_text(&mut o, "dsp", "device cycles per model instruction", s);
    }
    if let Some(s) = &c.cf {
        section_text(&mut o, "cf", "device time per model time", s);
    }
    for r in &c.refused {
        let _ = writeln!(o, "refused {} {} {}: {}", r.section, r.case, r.id, r.reason);
    }
    for w in &c.warnings {
        let _ = writeln!(o, "warning: {w}");
    }
    o
}

/// Estimated device cycles of the DSP rows of a model profile (instructions) with the calibration, as JSON and text.
pub fn estimate(c: &Calibration, profile: &Value) -> Result<(Value, String), String> {
    let (_, side) = read_side(profile)?;
    let s = c.dsp.as_ref().ok_or("calibration has no dsp section")?;
    let block = c.key.block_cycles.map(|b| b as f64);
    let mut rows = Vec::new();
    let mut o = String::new();
    let _ = writeln!(
        o,
        "# estimated device cycles (fit {:.4}, budget {:.4}; error: mean {}, worst under {:.2}%)",
        s.factor.fit,
        s.factor.budget,
        pct(s.error.mean),
        s.error.worst_under * 100.0
    );
    for (row, r) in &side.rows {
        if r.calls == 0 {
            continue;
        }
        let fit = s.factor.fit * r.max_block as f64;
        let bud = s.factor.budget * r.max_block as f64;
        let share = block.map(|b| bud / b);
        let _ = writeln!(
            o,
            "row {row:>2}: {:>9} insn/block -> {:>10.0} cycles ({:.2}% low), budget {:>10.0}{}",
            r.max_block,
            fit,
            s.error.worst_under * 100.0,
            bud,
            share.map_or(String::new(), |x| format!(
                " = {:.2}% of a block",
                x * 100.0
            )),
        );
        rows.push(serde_json::json!({
            "row": row, "max_block_insn": r.max_block, "est_cycles": fit,
            "est_cycles_budget": bud, "error_under": s.error.worst_under,
            "pct_of_block": share,
        }));
    }
    Ok((serde_json::json!({ "estimate": rows }), o))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(r: u64, calls: u64, sum: u64, call: u64, block: u64) -> Value {
        json!({"row": r, "kind": if r < 8 {"machine"} else {"effect"}, "calls": calls,
               "sum_cycles": sum, "max_call": call, "max_block": block})
    }

    fn prof(
        target: &str,
        case: &str,
        unit: &str,
        blocks: u64,
        stock: u64,
        rows: Vec<Value>,
        spans: Value,
    ) -> Value {
        json!({
            "schema": "dk.profile/1",
            "run": {"id": "x", "target": target, "firmware_sha256": "ff", "module_sha256": ["b", "a"],
                    "case_id": case, "input_sha256": "11"},
            "clock": {"source": "s", "unit": unit, "sampling": ""},
            "raw": [],
            "dsp": {"unit": unit, "blocks": blocks, "max_stock_block_delta": stock, "overruns": 0},
            "dsp_rows": rows,
            "spans": spans, "span_clock_hz": 1_000_000,
        })
    }

    fn req(d: Vec<Value>, m: Vec<Value>) -> Request {
        Request {
            device: d,
            model: m,
            block_cycles: Some(666_000),
            ..Request::default()
        }
    }

    /// The device counts `f` cycles per model instruction on every row.
    fn pair_for(f: f64, case: &str, rows: &[(u64, u64, u64)]) -> (Value, Value) {
        let d = rows
            .iter()
            .map(|&(r, call, block)| {
                row(
                    r,
                    10,
                    (call as f64 * f) as u64 * 10,
                    (call as f64 * f) as u64,
                    (block as f64 * f) as u64,
                )
            })
            .collect();
        let m = rows
            .iter()
            .map(|&(r, call, block)| row(r, 10, call * 10, call, block))
            .collect();
        (
            prof("dn2", case, "cycles", 5, 400_000, d, json!([])),
            prof("dn2", case, "insn", 5, 300_000, m, json!([])),
        )
    }

    #[test]
    fn a_known_factor_is_recovered_and_the_stock_is_the_header_minus_the_rows() {
        let (d, m) = pair_for(2.0, "shot", &[(0, 1000, 3000), (8, 500, 900)]);
        let c = fit(&req(vec![d], vec![m])).unwrap();
        let s = c.dsp.unwrap();
        assert!((s.factor.fit - 2.0).abs() < 1e-9, "{}", s.factor.fit);
        assert!((s.factor.budget - 2.0).abs() < 1e-9);
        assert!(s.error.mean.abs() < 1e-9 && s.error.worst_under < 1e-9);
        assert_eq!(s.pairs.iter().filter(|p| p.stat == "mean").count(), 2);
        assert_eq!(s.steady.n, 2);
        // 400000 - (6000 + 1800)
        assert_eq!(s.stock.unwrap().max, 392_200.0);
        assert!(c.refused.is_empty() && c.warnings.is_empty());
    }

    #[test]
    fn the_budget_factor_is_the_worst_ratio_and_the_fit_underpredicts_the_worst_row() {
        let (d1, m1) = pair_for(2.0, "a", &[(0, 1000, 1000)]);
        let (d2, m2) = pair_for(2.2, "b", &[(1, 1000, 1000)]);
        let c = fit(&req(vec![d1, d2], vec![m1, m2])).unwrap();
        let s = c.dsp.unwrap();
        assert!((s.factor.budget - 2.2).abs() < 0.001);
        assert!(s.factor.fit > 2.0 && s.factor.fit < 2.2);
        assert!(
            s.error.worst_under > 0.0,
            "the fit underpredicts the 2.2 row"
        );
        // the budget factor covers every measured pair
        assert!(s.pairs.iter().all(|p| s.factor.budget >= p.ratio - 1e-12));
        // leave-one-out: the budget of the other row does not cover the 2.2 row
        assert!(s.error.loo_budget_under > 0.05);
        assert!(s.error.loo_worst >= s.error.worst_under);
        // spread 10% exactly is not over the limit; this one is 10.0%
        assert!((s.spread - 0.1).abs() < 0.001);
    }

    #[test]
    fn a_spread_over_ten_percent_is_said() {
        let (d1, m1) = pair_for(2.0, "a", &[(0, 1000, 1000)]);
        let (d2, m2) = pair_for(2.5, "b", &[(1, 1000, 1000)]);
        let c = fit(&req(vec![d1, d2], vec![m1, m2])).unwrap();
        assert!(
            c.warnings.iter().any(|w| w.contains("spread")),
            "{:?}",
            c.warnings
        );
        assert!(text(&c).contains("warning: DSP spread"));
    }

    #[test]
    fn rows_cases_and_calls_that_do_not_pair_are_refused_with_a_reason() {
        let (d, m) = pair_for(2.0, "ok", &[(0, 100, 100)]);
        // unpaired row on the device, and a call-count mismatch on row 1
        let mut dd = d.clone();
        dd["dsp_rows"]
            .as_array_mut()
            .unwrap()
            .push(row(2, 10, 100, 10, 10));
        dd["dsp_rows"]
            .as_array_mut()
            .unwrap()
            .push(row(1, 10, 100, 10, 10));
        let mut mm = m.clone();
        mm["dsp_rows"]
            .as_array_mut()
            .unwrap()
            .push(row(1, 12, 100, 10, 10));
        // a case with a different input hash, and a case without a model
        let (d2, mut m2) = pair_for(2.0, "badkey", &[(0, 100, 100)]);
        m2["run"]["input_sha256"] = json!("22");
        let (d3, _) = pair_for(2.0, "alone", &[(0, 100, 100)]);
        let c = fit(&req(vec![dd, d2, d3], vec![mm, m2])).unwrap();
        let why = |case: &str, id: &str| {
            c.refused
                .iter()
                .find(|r| r.case == case && r.id == id)
                .map(|r| r.reason.clone())
                .unwrap_or_default()
        };
        assert!(why("ok", "row 2").starts_with("unpaired"));
        assert!(why("ok", "row 1").starts_with("call-count mismatch"));
        assert!(why("badkey", "").starts_with("key mismatch"));
        assert!(why("alone", "").starts_with("unpaired"));
        assert_eq!(c.dsp.unwrap().pairs.len(), 3, "only row 0 of case ok pairs");
    }

    #[test]
    fn calls_per_block_not_calls_are_compared() {
        let (mut d, m) = pair_for(2.0, "c", &[(0, 100, 100)]);
        // the device ran twice as many blocks and calls: same rate
        d["dsp"]["blocks"] = json!(10);
        d["dsp_rows"][0]["calls"] = json!(20);
        d["dsp_rows"][0]["sum_cycles"] = json!(20 * 200);
        let c = fit(&req(vec![d], vec![m])).unwrap();
        assert!(c.refused.is_empty(), "{:?}", c.refused);
    }

    #[test]
    fn a_missing_case_id_or_nothing_to_pair_is_an_error() {
        let (mut d, m) = pair_for(2.0, "c", &[(0, 100, 100)]);
        d["run"].as_object_mut().unwrap().remove("case_id");
        assert!(
            fit(&req(vec![d], vec![m.clone()]))
                .unwrap_err()
                .contains("case_id")
        );
        let (d2, _) = pair_for(2.0, "other", &[(0, 100, 100)]);
        let e = fit(&req(vec![d2], vec![m])).unwrap_err();
        assert!(e.contains("no pair") && e.contains("unpaired"), "{e}");
    }

    #[test]
    fn a_leave_one_out_needs_two_rows_and_flags_the_odd_one() {
        let rows = [(0, 1000, 1000), (1, 1000, 1000), (2, 1000, 1000)];
        let (d, m) = pair_for(2.0, "x", &rows);
        let mut d = d;
        // row 2 costs 20% more per instruction
        d["dsp_rows"][2]["max_call"] = json!(2400);
        d["dsp_rows"][2]["max_block"] = json!(2400);
        let c = fit(&req(vec![d], vec![m])).unwrap();
        let e = c.dsp.unwrap().error;
        assert!(e.loo_worst > 0.15, "{e:?}");
    }

    #[test]
    fn an_emulator_against_an_emulator_gives_exactly_one() {
        let rows = vec![row(0, 7, 7000, 1100, 3300), row(9, 3, 900, 400, 900)];
        let spans = json!([
            {"owner": 1, "kind": "module", "name": "m", "calls": 40, "sum_ticks": 123_457, "max_ticks": 9_001},
            {"owner": 0x101, "kind": "phase", "name": "late", "calls": 40, "sum_ticks": 77_777, "max_ticks": 3_333}
        ]);
        let m = prof("dt2", "e", "insn", 9, 600_000, rows, spans);
        let mut d = m.clone();
        d["dsp"]["unit"] = json!("insn");
        let c = fit(&req(vec![d], vec![m])).unwrap();
        for s in [c.dsp.as_ref().unwrap(), c.cf.as_ref().unwrap()] {
            assert_eq!(s.factor.fit, 1.0);
            assert_eq!(s.factor.budget, 1.0);
            assert_eq!(s.error.worst_under, 0.0);
            assert_eq!(s.error.mean, 0.0);
            assert_eq!(s.spread, 0.0);
        }
        assert!(
            c.warnings
                .iter()
                .any(|w| w.contains("emulator-against-emulator"))
        );
    }

    #[test]
    fn colfire_spans_compare_time_so_two_clocks_give_the_ratio_of_time() {
        let sp = |ticks: u64| json!([{"owner": 1, "kind": "module", "name": "m", "calls": 10, "sum_ticks": ticks * 10, "max_ticks": ticks}]);
        let mut d = prof("dn2", "s", "cycles", 1, 0, vec![], sp(3000));
        let m = prof("dn2", "s", "insn", 1, 0, vec![], sp(1000));
        d["span_clock_hz"] = json!(3_000_000);
        // 3000 ticks at 3 MHz = 1000 us; 1000 ticks at 1 MHz = 1000 us
        let c = fit(&req(vec![d.clone()], vec![m.clone()])).unwrap();
        assert!((c.cf.unwrap().factor.fit - 1.0).abs() < 1e-12);
        // fewer calls on the device are refused
        d["spans"][0]["calls"] = json!(9);
        let e = fit(&req(vec![d], vec![m])).unwrap_err();
        assert!(e.contains("call-count mismatch"), "{e}");
    }

    #[test]
    fn the_file_round_trips_and_the_estimate_reads_a_model_profile() {
        let (d, m) = pair_for(2.0, "shot", &[(0, 1000, 3000)]);
        let c = fit(&req(vec![d], vec![m.clone()])).unwrap();
        let back: Calibration = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back, c);
        assert!(text(&c).contains("factor fit 2.0000"));
        let (j, t) = estimate(&c, &m).unwrap();
        assert_eq!(j["estimate"][0]["est_cycles"], 6000.0);
        assert!(t.contains("% of a block"), "{t}");
    }

    #[test]
    fn a_profile_without_dsp_rows_is_read_without_error() {
        let p =
            json!({"schema": "dk.profile/1", "run": {"id": "x", "target": "dn2", "case_id": "c"}});
        let (case, s) = read_side(&p).unwrap();
        assert_eq!(case.as_deref(), Some("c"));
        assert!(s.rows.is_empty() && s.spans.is_empty());
    }
}
