//! The rule for the records of a built module: for each record kind, the validator of the service that owns the kind.
//!
//! `dkneeds::KINDS` names the owner of each kind (`[records.kinds]` of sdk/sdk.toml). The services are Rust crates and the map from an owner's name to its crate is written by hand
//! below, one line each (`owner_rule`); the test at the end fails for an owner without a line. Each line names the free `validate` of the service's `records.rs` (`dkrt_svc_lfo::records::validate`), which is the function the
//! service's `Records::validate` runs, so patchkit needs no core and no host fake to call it.
//!
//! What the validators cover is what the records' own bytes decide: each service's `validate` module names its checks. What needs the device, the module's image, another record kind or the registry
//! state stays in the service's `check` at load, so a module that passes here can still be refused at load for those. Not for wasm32: the services are host and ColdFire crates.
use dkrt_abi::service::Problem;

type Rule = fn(u32, &[&[u8]], &mut dyn FnMut(Problem));

/// The services that own record kinds, by the name of `[[service]]` in sdk/sdk.toml. Add a line when a service gets a kind: the test below fails until then.
fn owner_rule(owner: &str) -> Option<Rule> {
    match owner {
        "lfo" => Some(dkrt_svc_lfo::records::validate),
        "shortcuts" => Some(dkrt_svc_shortcuts::records::validate),
        "fx" => Some(dkrt_svc_fx::records::validate),
        "machines" => Some(dkrt_svc_machines::records::validate),
        _ => None,
    }
}

/// The name of the owner of record kind `kind`.
pub fn owner_of(kind: u32) -> Option<&'static str> {
    dkneeds::KINDS.iter().find(|k| k.0 == kind).map(|k| k.2)
}

/// Runs the owner's rule over the bodies of one kind (in file order). A kind no service owns, or an owner with no entry here, is a problem of its own.
pub fn validate(kind: u32, bodies: &[&[u8]]) -> Vec<Problem> {
    let mut out = vec![];
    match owner_of(kind).and_then(owner_rule) {
        Some(rule) => rule(kind, bodies, &mut |p| out.push(p)),
        None => out.push(Problem::set(
            kind,
            dkrt_sdk::E_FORMAT,
            "no service owns this record kind",
        )),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_record_kind_has_an_owner_with_a_rule() {
        for (kind, _, owner) in dkneeds::KINDS {
            assert!(
                owner_rule(owner).is_some(),
                "kind {kind}: add the service {owner:?} to owner_rule"
            );
        }
    }

    #[test]
    fn the_owner_of_a_kind_judges_its_records() {
        // an lfo_shape set of no records is refused by the lfo service, a machines kind set of none is accepted
        assert_eq!(validate(dkneeds::KIND_LFO_SHAPE, &[]).len(), 1);
        assert!(validate(dkneeds::KIND_PARAM, &[]).is_empty());
        assert_eq!(validate(999, &[]).len(), 1);
    }
}
