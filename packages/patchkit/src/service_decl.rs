//! `[service]` of a manifest: what a service declares of its own capacity and records (docs/plans/runtime-contract-and-lifecycle.md, section 5).
//! The checks run where a manifest is validated (`module check`, `service-check`, `patchkit gen`); nothing is packed from them yet.

use crate::error::*;
use crate::service_record::ServiceRecord;
use crate::service_resource::{Aggregate, CORE_RESOURCES, Quantity, ServiceResource};
use serde::Deserialize;
use std::collections::BTreeSet;

/// The `[service]` table: `[[service.resource]]` and `[[service.record]]`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ServiceDecl {
    #[serde(default)]
    pub resource: Vec<ServiceResource>,
    #[serde(default)]
    pub record: Vec<ServiceRecord>,
}

impl ServiceDecl {
    pub fn is_empty(&self) -> bool {
        self.resource.is_empty() && self.record.is_empty()
    }

    /// Checks one service's declarations: unique names and kinds, `profile:` keys that every runtime device file has, `draws_from` that resolves,
    /// `scale` only with `max`. `has_key` says whether a profile key exists.
    pub fn validate(&self, id: &str, has_key: &dyn Fn(&str) -> bool) -> Result<()> {
        let mut names = BTreeSet::new();
        for r in &self.resource {
            let who = format!("{id}: [[service.resource]] {:?}", r.name);
            if !valid_name(&r.name) {
                return Err(err!(
                    "{who}: use lowercase ascii letters, digits and `_`, starting with a letter"
                ));
            }
            if !names.insert(r.name.as_str()) {
                return Err(err!("{who} is declared twice"));
            }
            match &r.limit {
                None | Some(Quantity::Number(_)) => {}
                Some(Quantity::Text(t)) if t == "init" => {}
                Some(Quantity::Text(t)) => profile_key(&who, "limit", t, has_key)?,
            }
            match (&r.scale, r.aggregate) {
                (None, _) => {}
                (Some(_), Aggregate::Sum) => {
                    return Err(err!("{who}: `scale` belongs to `aggregate = \"max\"`"));
                }
                (Some(Quantity::Number(_)), Aggregate::Max) => {}
                (Some(Quantity::Text(t)), Aggregate::Max) => {
                    profile_key(&who, "scale", t, has_key)?
                }
            }
        }
        for r in &self.resource {
            let Some(from) = &r.draws_from else { continue };
            let who = format!("{id}: [[service.resource]] {:?}", r.name);
            let ok = match from.strip_prefix("core.") {
                Some(core) => CORE_RESOURCES.contains(&core),
                None => from != &r.name && names.contains(from.as_str()),
            };
            if !ok {
                return Err(err!(
                    "{who}: draws_from {from:?} is neither `core.<name>` ({}) nor another resource of this service",
                    CORE_RESOURCES.join(", ")
                ));
            }
        }
        let (mut record_names, mut kinds) = (BTreeSet::new(), BTreeSet::new());
        for r in &self.record {
            let who = format!("{id}: [[service.record]] {:?}", r.name);
            if !valid_name(&r.name) {
                return Err(err!(
                    "{who}: use lowercase ascii letters, digits and `_`, starting with a letter"
                ));
            }
            if !record_names.insert(r.name.as_str()) {
                return Err(err!("{who} is declared twice"));
            }
            if !kinds.insert(r.kind) {
                return Err(err!("{who}: kind {} is used twice", r.kind));
            }
        }
        Ok(())
    }
}

fn valid_name(s: &str) -> bool {
    s.bytes().next().is_some_and(|c| c.is_ascii_lowercase())
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

/// `text` of `field` must be `profile:<key>` with a key of the device files.
fn profile_key(who: &str, field: &str, text: &str, has_key: &dyn Fn(&str) -> bool) -> Result<()> {
    match text.strip_prefix("profile:") {
        Some(key) if has_key(key) => Ok(()),
        Some(key) => Err(err!(
            "{who}: {field} {text:?}: no integer `{key}` in [limits] or [digikit] of every runtime device file (devices/*.toml)"
        )),
        None => Err(err!(
            "{who}: {field} {text:?} is not a number, \"profile:<key>\"{}",
            if field == "limit" { " or \"init\"" } else { "" }
        )),
    }
}

#[cfg(test)]
mod tests;
