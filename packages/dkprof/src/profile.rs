//! The artifact `dk.profile/1`. JSON is the artifact; the text of `profile report` is a view of it.
//!
//! A producer (the emulator today, the device later) fills `raw` with counters `{cpu, addr, count}` and keeps them as they are. Names, shares and
//! module sums are computed by [`crate::report`] from the raw counters and an address map, so a replay with a better map gives better names.

use dkaddrmap::{Cpu, LoadTable};
use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "dk.profile/1";

fn pc_series() -> String {
    "pc".into()
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Run {
    pub id: String,
    /// `dt2`, `dn2` or `host`.
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub firmware_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub module_sha256: Vec<String>,
    /// The calibration case this run measured (`shot-note60`, ...): a device run and its model run share it ([`crate::calibrate`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_id: Option<String>,
    /// Hash of the input of the case (project, notes, sample), so two runs of one case id are known to have played the same thing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_revision: Option<String>,
    /// The emulator binary or device build that made the counters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary_sha256: Option<String>,
    /// Where each module sat during the run (read from the runtime's module table); lets a reader name module addresses without the device.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loads: Option<LoadTable>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Clock {
    /// What advances the counts, for example `emu.cf.insn` (emulated ColdFire instructions).
    pub source: String,
    /// The unit of `count` in the pc series, for example `insn`.
    pub unit: String,
    /// How the counters were taken, for example `every 64 insns in run_fast, every insn in step_once`.
    pub sampling: String,
}

/// A counter: `count` units of the clock at `addr` (ColdFire byte address, SHARC software PC).
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Raw {
    pub cpu: Cpu,
    pub addr: u64,
    pub count: u64,
    /// `pc` (sampled program counters), or a producer's own series such as `idle_pc` and `exit_pc`. A series is never added to another.
    #[serde(default = "pc_series")]
    pub series: String,
}

/// A measurement: a value, or N/A with the reason. Never a zero for "not available".
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Metric {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    pub unit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub na_reason: Option<String>,
}

impl Metric {
    pub fn value(name: &str, value: f64, unit: &str) -> Metric {
        Metric {
            name: name.into(),
            value: Some(value),
            unit: unit.into(),
            na_reason: None,
        }
    }
    pub fn na(name: &str, unit: &str, reason: &str) -> Metric {
        Metric {
            name: name.into(),
            value: None,
            unit: unit.into(),
            na_reason: Some(reason.into()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Marker {
    pub name: String,
    /// In units of the clock.
    pub at: u64,
}

/// Counters the producer could not keep (a truncated table, a full buffer).
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Loss {
    pub series: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dropped: Option<u64>,
}

/// A runtime span: `calls` runs of one owner (a module, a phase or a poll) that took `sum_ticks` in all, the longest `max_ticks`.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Span {
    /// The runtime owner id: a module record, `0x100 | phase`, `0x200 | poll`.
    pub owner: u32,
    /// `module`, `phase` or `poll`.
    pub kind: String,
    pub name: String,
    pub calls: u64,
    pub sum_ticks: u64,
    pub max_ticks: u64,
}

/// One row of the DSP stats window (P8d): a DSP dispatch row (0 to 7 machines, 8 to 15 effects) with its counters in `Dsp::unit`.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DspRow {
    pub row: u32,
    /// `machine` or `effect`.
    pub kind: String,
    pub calls: u64,
    pub sum_cycles: u64,
    pub max_call: u64,
    pub max_block: u64,
}

/// The header of the DSP stats window: what the rows were counted over.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Dsp {
    /// `cycles` on a device, `insn` (EMUCLK, instructions) on the emulator.
    pub unit: String,
    pub blocks: u64,
    /// The largest block of the whole DSP (stock work included) in `unit`.
    pub max_stock_block_delta: u64,
    /// Blocks at or over the block period.
    pub overruns: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Profile {
    pub schema: String,
    pub run: Run,
    pub clock: Clock,
    pub raw: Vec<Raw>,
    #[serde(default)]
    pub metrics: Vec<Metric>,
    #[serde(default)]
    pub markers: Vec<Marker>,
    #[serde(default)]
    pub loss: Vec<Loss>,
    /// What measuring cost, as metrics (a value or N/A with a reason).
    #[serde(default)]
    pub overhead: Vec<Metric>,
    /// Runtime span counters (a producer on the device); ticks run at `span_clock_hz`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spans: Vec<Span>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_clock_hz: Option<u64>,
    /// The ticks of the span clock the counters were gathered over (the window of the recording): `sum_ticks / span_elapsed_ticks` is the share of the CPU.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_elapsed_ticks: Option<u64>,
    /// The DSP stats window (a producer on the device or the emulator): one row per DSP dispatch row, and its header.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dsp_rows: Vec<DspRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dsp: Option<Dsp>,
}

impl Profile {
    pub fn parse(text: &str) -> Result<Profile, String> {
        let p: Profile = serde_json::from_str(text).map_err(|e| format!("profile: {e}"))?;
        p.check()?;
        Ok(p)
    }

    pub fn check(&self) -> Result<(), String> {
        if self.schema != SCHEMA {
            return Err(format!(
                "profile: schema {:?}, expected {SCHEMA}",
                self.schema
            ));
        }
        for m in self.metrics.iter().chain(&self.overhead) {
            match (m.value, &m.na_reason) {
                (Some(_), None) | (None, Some(_)) => {}
                (Some(_), Some(_)) => {
                    return Err(format!(
                        "profile: metric {} has a value and an N/A reason",
                        m.name
                    ));
                }
                (None, None) => {
                    return Err(format!(
                        "profile: metric {} has neither a value nor an N/A reason (N/A is never a zero)",
                        m.name
                    ));
                }
            }
        }
        if !self.spans.is_empty() && !matches!(self.span_clock_hz, Some(hz) if hz > 0) {
            return Err("profile: spans need span_clock_hz greater than zero".into());
        }
        for s in &self.spans {
            if !matches!(s.kind.as_str(), "module" | "phase" | "poll") {
                return Err(format!(
                    "profile: span {} has kind {:?}, expected module, phase or poll",
                    s.name, s.kind
                ));
            }
            if s.max_ticks > s.sum_ticks {
                return Err(format!(
                    "profile: span {} has max_ticks above sum_ticks",
                    s.name
                ));
            }
            if s.calls == 0 && s.sum_ticks != 0 {
                return Err(format!("profile: span {} has ticks but no calls", s.name));
            }
        }
        if !self.dsp_rows.is_empty() && self.dsp.is_none() {
            return Err("profile: dsp_rows need the dsp header (unit, blocks)".into());
        }
        for r in &self.dsp_rows {
            let want = match r.row {
                0..=7 => "machine",
                8..=15 => "effect",
                _ => return Err(format!("profile: dsp row {} is past 15", r.row)),
            };
            if r.kind != want {
                return Err(format!(
                    "profile: dsp row {} has kind {:?}, expected {want}",
                    r.row, r.kind
                ));
            }
            if r.max_call > r.max_block || r.max_block > r.sum_cycles {
                return Err(format!(
                    "profile: dsp row {} needs max_call <= max_block <= sum_cycles",
                    r.row
                ));
            }
            if r.calls == 0 && r.sum_cycles != 0 {
                return Err(format!(
                    "profile: dsp row {} has cycles but no calls",
                    r.row
                ));
            }
        }
        Ok(())
    }

    pub fn to_json(&self) -> String {
        let mut s = serde_json::to_string(self).expect("profile serialises");
        s.push('\n');
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD: &str = r#"{"schema":"dk.profile/1","run":{"id":"t","target":"dn2"},"clock":{"source":"s","unit":"insn","sampling":"x"},"raw":[],"metrics":[],"markers":[],"loss":[],"overhead":[]}"#;

    fn span(kind: &str, calls: u64, sum: u64, max: u64) -> Span {
        Span {
            owner: 0x100,
            kind: kind.into(),
            name: "early".into(),
            calls,
            sum_ticks: sum,
            max_ticks: max,
        }
    }

    fn with(spans: Vec<Span>, hz: Option<u64>) -> Profile {
        let mut p = Profile::parse(OLD).unwrap();
        p.spans = spans;
        p.span_clock_hz = hz;
        p
    }

    #[test]
    fn spans_round_trip() {
        let mut p = with(vec![span("phase", 4, 100, 40)], Some(1_000_000));
        p.span_elapsed_ticks = Some(5_000);
        let back = Profile::parse(&p.to_json()).unwrap();
        assert_eq!(back, p);
    }

    fn dsp_row(row: u32, kind: &str, calls: u64, sum: u64, call: u64, block: u64) -> DspRow {
        DspRow {
            row,
            kind: kind.into(),
            calls,
            sum_cycles: sum,
            max_call: call,
            max_block: block,
        }
    }

    fn with_dsp(rows: Vec<DspRow>, header: bool) -> Profile {
        let mut p = Profile::parse(OLD).unwrap();
        p.dsp_rows = rows;
        if header {
            p.dsp = Some(Dsp {
                unit: "cycles".into(),
                blocks: 1500,
                max_stock_block_delta: 666_000,
                overruns: 0,
            });
        }
        p
    }

    #[test]
    fn dsp_rows_round_trip_and_an_old_file_has_none() {
        let p = with_dsp(vec![dsp_row(9, "effect", 4, 2u64 << 32, 900, 4000)], true);
        assert_eq!(Profile::parse(&p.to_json()).unwrap(), p);
        assert!(Profile::parse(OLD).unwrap().dsp_rows.is_empty());
    }

    #[test]
    fn check_rejects_bad_dsp_rows() {
        assert!(
            with_dsp(vec![dsp_row(0, "machine", 1, 9, 3, 5)], true)
                .check()
                .is_ok()
        );
        assert!(
            with_dsp(vec![dsp_row(0, "machine", 1, 9, 3, 5)], false)
                .check()
                .is_err()
        );
        assert!(
            with_dsp(vec![dsp_row(16, "effect", 1, 9, 3, 5)], true)
                .check()
                .is_err()
        );
        assert!(
            with_dsp(vec![dsp_row(3, "effect", 1, 9, 3, 5)], true)
                .check()
                .is_err()
        );
        assert!(
            with_dsp(vec![dsp_row(8, "machine", 1, 9, 3, 5)], true)
                .check()
                .is_err()
        );
        assert!(
            with_dsp(vec![dsp_row(0, "machine", 1, 9, 6, 5)], true)
                .check()
                .is_err()
        );
        assert!(
            with_dsp(vec![dsp_row(0, "machine", 1, 4, 3, 5)], true)
                .check()
                .is_err()
        );
        assert!(
            with_dsp(vec![dsp_row(0, "machine", 0, 4, 0, 0)], true)
                .check()
                .is_err()
        );
    }

    #[test]
    fn old_file_without_spans_is_unchanged() {
        let p = Profile::parse(OLD).unwrap();
        assert!(p.spans.is_empty() && p.span_clock_hz.is_none());
        assert_eq!(p.to_json().trim_end(), OLD);
    }

    #[test]
    fn check_rejects_bad_spans() {
        assert!(with(vec![span("bogus", 1, 2, 2)], Some(1)).check().is_err());
        assert!(with(vec![span("poll", 1, 2, 2)], None).check().is_err());
        assert!(with(vec![span("poll", 1, 2, 2)], Some(0)).check().is_err());
        assert!(with(vec![span("poll", 2, 2, 3)], Some(1)).check().is_err());
        assert!(with(vec![span("poll", 0, 2, 2)], Some(1)).check().is_err());
        assert!(with(vec![span("poll", 0, 0, 0)], Some(1)).check().is_ok());
    }
}
