//! The rule for the records of a built module: for each record kind, the `validate_records` of the service that owns the kind.
//!
//! `dkneeds::KINDS` names the owner of each kind (`[records.kinds]` of sdk/sdk.toml). The services are Rust crates, so the owner's name selects the crate here, one line each.
//! The service's own `check` runs the same function when the runtime loads the module, so a module that passes here is not refused for its records' content at load.
//! Not for wasm32: the services are host and ColdFire crates.
use dkrt_abi::service::{Problem, Service};
use dkrt_test_support::FakeCore;

type Rule = fn(u32, &[&[u8]], &mut dyn FnMut(Problem));

fn rule_of<S: Service<FakeCore>>() -> Rule {
    fn run<S: Service<FakeCore>>(kind: u32, bodies: &[&[u8]], report: &mut dyn FnMut(Problem)) {
        S::validate_records(kind, bodies, &mut |p| report(p));
    }
    run::<S>
}

/// The services that own record kinds, by the name of `[[service]]` in sdk/sdk.toml. Add a line when a service gets a kind: the test below fails until then.
fn owner_rule(owner: &str) -> Option<Rule> {
    match owner {
        "lfo" => Some(rule_of::<dkrt_svc_lfo::records::Lfo>()),
        "fx" => Some(rule_of::<dkrt_svc_fx::records::Fx>()),
        "machines" => Some(rule_of::<dkrt_svc_machines::records::Machines>()),
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
