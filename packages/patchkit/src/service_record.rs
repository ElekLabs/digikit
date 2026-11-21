//! `[[service.record]]` of a service manifest: a record kind the service owns, local to it (docs/plans/runtime-contract-and-lifecycle.md, section 5).
//! Parsed and validated only: the packer does not write it yet (RC3). Not the top-level `[[record]]` of a module, which names one record a module emits.

use serde::Deserialize;

/// One `[[service.record]]`: the packed kind is (service hash, `kind`).
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ServiceRecord {
    pub name: String,
    /// The local kind, unique in the service.
    pub kind: u16,
    /// The record body in bytes.
    pub len: u32,
}
