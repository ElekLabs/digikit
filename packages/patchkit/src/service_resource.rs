//! `[[service.resource]]` of a service manifest (docs/plans/runtime-contract-and-lifecycle.md, sections 5 and 6.5): a capacity the service owns.
//! Parsed and validated only: the packer does not write it yet (RC3).

use serde::Deserialize;

/// A number in the manifest or a name that the check resolves: `limit` is a number, `"profile:<key>"` or `"init"`; `scale` is a number or `"profile:<key>"`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum Quantity {
    Number(u64),
    Text(String),
}

/// How the units of one resource add up.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Aggregate {
    /// The units of all packages add up (the default).
    #[default]
    Sum,
    /// The largest unit counts, times `scale`.
    Max,
}

/// One `[[service.resource]]`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ServiceResource {
    pub name: String,
    /// The capacity: a number, `"profile:<key>"` (a key of `[limits]` or `[digikit]` in `devices/*.toml`) or `"init"` (the service declares it in `start`).
    #[serde(default)]
    pub limit: Option<Quantity>,
    #[serde(default)]
    pub aggregate: Aggregate,
    /// Only with `aggregate = "max"`: the factor of the largest unit.
    #[serde(default)]
    pub scale: Option<Quantity>,
    /// `"core.<name>"` or another resource of the same service: the units also count against it.
    #[serde(default)]
    pub draws_from: Option<String>,
}

/// The resources of the core that a service resource can draw from (section 6.5).
pub const CORE_RESOURCES: [&str; 7] = [
    "modules", "arena", "stage", "dsp_area", "dsp_cost", "store", "settings",
];
