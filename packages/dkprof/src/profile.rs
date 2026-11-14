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
        Ok(())
    }

    pub fn to_json(&self) -> String {
        let mut s = serde_json::to_string(self).expect("profile serialises");
        s.push('\n');
        s
    }
}
