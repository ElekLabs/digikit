extern crate std;

use super::*;
use crate::check_selection;
use serde_json::Value;
use std::vec::Vec;

fn pkg(n: u8) -> OwnerId {
    OwnerId(n)
}

fn key(owner: Owner, name: u32) -> PoolKey {
    PoolKey { owner, name }
}

const TOKENS: u32 = 1;
const LIMITS: u32 = 2;
const VOICE: u32 = 10;
const FX: u32 = 11;
const DSP: u32 = 12;
const JUDGED: usize = 11;
const SKIPPED: usize = 37;

fn sum(name: u32, limit: u32) -> ResourceDecl {
    ResourceDecl {
        name,
        limit,
        aggregate: Aggregate::Sum,
        draws_from: None,
    }
}

fn want(pool: PoolKey, units: u32) -> Want {
    Want { pool, units }
}

/// The service package 1 owns `ref.tokens` (limit 2).
fn with_tokens() -> (ResourceLedger, PoolKey) {
    let mut l = ResourceLedger::new();
    l.declare(Owner::Package(pkg(1)), &sum(TOKENS, 2)).unwrap();
    (l, key(Owner::Package(pkg(1)), TOKENS))
}

#[test]
fn claim_over_limit_is_refused_and_claims_nothing() {
    let (mut l, tokens) = with_tokens();
    let other = key(Owner::Core, LIMITS);
    l.declare(Owner::Core, &sum(LIMITS, 1)).unwrap();
    l.claim(pkg(2), &[want(tokens, 1)]).unwrap();
    // the first want would fit, the second does not: neither is taken
    let short = l
        .claim(pkg(3), &[want(tokens, 1), want(other, 2)])
        .unwrap_err();
    assert_eq!(
        short,
        ClaimError::Short(PoolShort {
            pool: other,
            need: 2,
            free: 1
        })
    );
    assert_eq!(l.fits(&[want(tokens, 1)]), Ok(()));
    assert_eq!(
        l.fits(&[want(tokens, 2)]),
        Err(PoolShort {
            pool: tokens,
            need: 2,
            free: 1
        })
    );
    // a pool nobody declared is short with nothing free
    let ghost = key(Owner::Core, 99);
    assert_eq!(
        l.fits(&[want(ghost, 1)]),
        Err(PoolShort {
            pool: ghost,
            need: 1,
            free: 0
        })
    );
}

#[test]
fn fits_changes_nothing() {
    let (mut l, tokens) = with_tokens();
    for _ in 0..5 {
        l.fits(&[want(tokens, 2)]).unwrap();
    }
    l.claim(pkg(2), &[want(tokens, 2)]).unwrap();
}

#[test]
fn release_returns_units() {
    let (mut l, tokens) = with_tokens();
    l.claim(pkg(2), &[want(tokens, 2)]).unwrap();
    assert!(l.claim(pkg(3), &[want(tokens, 1)]).is_err());
    l.release(pkg(2));
    l.claim(pkg(3), &[want(tokens, 2)]).unwrap();
    // releasing a package that claimed nothing is harmless
    l.release(pkg(9));
}

#[test]
fn a_consumer_that_claims_again_adds_to_its_claim() {
    let (mut l, tokens) = with_tokens();
    l.claim(pkg(2), &[want(tokens, 1)]).unwrap();
    assert!(l.claim(pkg(2), &[want(tokens, 2)]).is_err());
    l.claim(pkg(2), &[want(tokens, 1)]).unwrap();
    l.release(pkg(2));
    assert_eq!(l.fits(&[want(tokens, 2)]), Ok(()));
}

#[test]
fn withdraw_is_refused_while_claimed() {
    let (mut l, tokens) = with_tokens();
    l.claim(pkg(2), &[want(tokens, 1)]).unwrap();
    assert_eq!(l.withdraw(Owner::Package(pkg(1))), Err(LedgerError::InUse));
    l.release(pkg(2));
    l.withdraw(Owner::Package(pkg(1))).unwrap();
    // the pool is gone, and can be declared again
    assert!(l.fits(&[want(tokens, 1)]).is_err());
    l.declare(Owner::Package(pkg(1)), &sum(TOKENS, 2)).unwrap();
}

#[test]
fn withdraw_is_refused_while_another_pool_draws_from_it() {
    let mut l = ResourceLedger::new();
    l.declare(Owner::Package(pkg(1)), &sum(DSP, 100)).unwrap();
    let child = ResourceDecl {
        draws_from: Some(key(Owner::Package(pkg(1)), DSP)),
        ..sum(FX, 100)
    };
    l.declare(Owner::Package(pkg(2)), &child).unwrap();
    assert_eq!(l.withdraw(Owner::Package(pkg(1))), Err(LedgerError::InUse));
    l.withdraw(Owner::Package(pkg(2))).unwrap();
    l.withdraw(Owner::Package(pkg(1))).unwrap();
}

#[test]
fn duplicate_declare_is_refused() {
    let (mut l, _) = with_tokens();
    assert_eq!(
        l.declare(Owner::Package(pkg(1)), &sum(TOKENS, 9)),
        Err(LedgerError::Duplicate)
    );
    // the same name of another owner is another pool
    l.declare(Owner::Package(pkg(2)), &sum(TOKENS, 9)).unwrap();
    l.declare(Owner::Core, &sum(TOKENS, 9)).unwrap();
    let orphan = ResourceDecl {
        draws_from: Some(key(Owner::Core, 77)),
        ..sum(FX, 1)
    };
    assert_eq!(
        l.declare(Owner::Core, &orphan),
        Err(LedgerError::UnknownParent)
    );
}

#[test]
fn draws_from_counts_against_parent() {
    let mut l = ResourceLedger::new();
    let parent = key(Owner::Core, DSP);
    l.declare(Owner::Core, &sum(DSP, 100)).unwrap();
    let a = key(Owner::Package(pkg(1)), FX);
    let b = key(Owner::Package(pkg(2)), FX);
    for (n, k) in [(1, a), (2, b)] {
        l.declare(
            Owner::Package(pkg(n)),
            &ResourceDecl {
                draws_from: Some(parent),
                ..sum(FX, 1000)
            },
        )
        .unwrap();
        assert_eq!(k.name, FX);
    }
    l.claim(pkg(3), &[want(a, 60)]).unwrap();
    // pool b has room of its own, the parent has 40 left
    assert_eq!(
        l.claim(pkg(4), &[want(b, 41)]),
        Err(ClaimError::Short(PoolShort {
            pool: parent,
            need: 41,
            free: 40
        }))
    );
    l.claim(pkg(4), &[want(b, 40)]).unwrap();
    // the parent's own claims count with its children's
    assert!(l.claim(pkg(5), &[want(parent, 1)]).is_err());
    l.release(pkg(3));
    l.claim(pkg(5), &[want(parent, 60)]).unwrap();
    let mut rows = [PoolReport {
        pool: parent,
        limit: 0,
        used: 0,
        largest: 0,
    }; 4];
    let n = l.report(&mut rows);
    assert_eq!(n, 3);
    assert_eq!(
        (rows[0].pool, rows[0].limit, rows[0].used, rows[0].largest),
        (parent, 100, 100, 0)
    );
    assert_eq!((rows[1].pool, rows[1].used, rows[1].largest), (a, 0, 1000));
    assert_eq!((rows[2].pool, rows[2].used, rows[2].largest), (b, 40, 960));
}

/// The pools of the voice rule: voices (`Max`, scaled by the voice count) and effects (`Sum`) both draw from the DSP pool.
fn dsp_ledger(voices: u32, headroom: u32) -> (ResourceLedger, PoolKey, PoolKey) {
    let mut l = ResourceLedger::new();
    let dsp = key(Owner::Core, DSP);
    l.declare(Owner::Core, &sum(DSP, headroom)).unwrap();
    let draw = Some(dsp);
    l.declare(
        Owner::Package(pkg(1)),
        &ResourceDecl {
            name: VOICE,
            limit: u32::MAX,
            aggregate: Aggregate::Max { scale: voices },
            draws_from: draw,
        },
    )
    .unwrap();
    l.declare(
        Owner::Package(pkg(2)),
        &ResourceDecl {
            draws_from: draw,
            ..sum(FX, u32::MAX)
        },
    )
    .unwrap();
    (
        l,
        key(Owner::Package(pkg(1)), VOICE),
        key(Owner::Package(pkg(2)), FX),
    )
}

#[test]
fn voice_rule_matches_check_selection_over_a_grid() {
    let machines = [[100u32, 250, 60], [100, 251, 0], [90, 240, 10]];
    let fx = [[0u32, 0], [0, 0], [25, 15]];
    let mut asked = 0;
    for (m, f) in machines.iter().zip(fx.iter()) {
        for (voices, headroom) in [(4u32, 1000u32), (4, 1004), (8, 500), (1, 250)] {
            let (mut l, voice, fxp) = dsp_ledger(voices, headroom);
            let mut fits = true;
            let mut taken = 3;
            for &c in m {
                taken += 1;
                fits &= l.claim(pkg(taken), &[want(voice, c)]).is_ok();
            }
            for &c in f {
                taken += 1;
                fits &= l.claim(pkg(taken), &[want(fxp, c)]).is_ok();
            }
            let machines: Vec<u32> = m.to_vec();
            let fx: Vec<u32> = f.to_vec();
            assert_eq!(
                fits,
                check_selection(voices, headroom, &machines, &fx).fits,
                "{m:?} {f:?} {voices} voices {headroom}"
            );
            asked += 1;
        }
    }
    assert_eq!(asked, 12);
}

/// The verdicts of the shared vectors (`runtime/core/vectors/fit-vectors.json`) for the DSP load: the cases and sets whose modules need only DSP cost.
/// The ledger is loaded with the capacity's start (`used`, `largest`), claims each module in turn and must refuse the same module with the same
/// need and free as the device rule.
#[test]
fn voice_rule_matches_check_selection() {
    let v: Value = serde_json::from_str(include_str!("../../../vectors/fit-vectors.json")).unwrap();
    let base = &v["caps_base"];
    let mut judged = 0;
    let mut skipped = 0;
    let all = v["cases"]
        .as_array()
        .unwrap()
        .iter()
        .chain(v["sets"].as_array().unwrap());
    for case in all {
        let modules: Vec<&Value> = match &case["needs"] {
            Value::Array(a) => a.iter().collect(),
            one => std::vec![one],
        };
        let only_dsp = |n: &Value| {
            n.as_object()
                .unwrap()
                .keys()
                .all(|k| matches!(k.as_str(), "DSP_COST_VOICE" | "DSP_COST_FX" | "times"))
        };
        let caps = case.get("caps").and_then(Value::as_object);
        let only_dsp_caps = caps.is_none_or(|c| {
            c.iter()
                .all(|(k, v)| k == "DSP_COST" || (k == "DSP_VOICES" && !v.is_null()))
        });
        if !modules.iter().all(|n| only_dsp(n)) || !only_dsp_caps {
            skipped += 1;
            continue;
        }
        let cap = |name: &str| {
            case.get("caps")
                .and_then(|c| c.get(name))
                .unwrap_or(&base[name])
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_u64().unwrap() as u32)
                .collect::<Vec<_>>()
        };
        let (headroom, used, largest) = {
            let c = cap("DSP_COST");
            (c[0], c[1], c[2])
        };
        let voices = cap("DSP_VOICES")[0];
        let (mut l, voice, fxp) = dsp_ledger(voices, headroom);
        // what the capacity already holds: the largest voice cost, and the effects as the rest of `used`
        let fx_used = used - largest * voices;
        l.claim(pkg(3), &[want(voice, largest)]).unwrap();
        l.claim(pkg(4), &[want(fxp, fx_used)]).unwrap();
        let mut first_short = None;
        let mut index = 0usize;
        let mut consumer = 5u8;
        'set: for n in &modules {
            for _ in 0..n.get("times").and_then(Value::as_u64).unwrap_or(1) {
                let g = |k: &str| n.get(k).and_then(Value::as_u64).unwrap_or(0) as u32;
                let mut wants = Vec::new();
                if g("DSP_COST_VOICE") > 0 {
                    wants.push(want(voice, g("DSP_COST_VOICE")));
                }
                if g("DSP_COST_FX") > 0 {
                    wants.push(want(fxp, g("DSP_COST_FX")));
                }
                if let Err(e) = l.claim(pkg(consumer), &wants) {
                    let ClaimError::Short(s) = e else {
                        panic!("{}: {e:?}", case["name"])
                    };
                    first_short = Some((index, s));
                    break 'set;
                }
                consumer += 1;
                index += 1;
            }
        }
        let is_set = case["want"].as_array().is_some_and(|w| w.len() == 4)
            || (case["want"].is_null() && case["needs"].is_array());
        match (&case["want"], first_short) {
            (Value::Null, None) => {}
            (Value::Array(w), Some((i, s))) => {
                let (need, free) = (
                    w[w.len() - 2].as_u64().unwrap(),
                    w[w.len() - 1].as_u64().unwrap(),
                );
                assert_eq!(s.pool, key(Owner::Core, DSP), "{}", case["name"]);
                assert_eq!(
                    (s.need as u64, s.free as u64),
                    (need, free),
                    "{}",
                    case["name"]
                );
                if is_set {
                    assert_eq!(i as u64, w[0].as_u64().unwrap(), "{}", case["name"]);
                }
            }
            (want, got) => panic!("{}: want {want}, ledger {got:?}", case["name"]),
        }
        judged += 1;
    }
    // the DSP cases and sets of the file are judged; the others need resources the ledger has no pool for here. A new vector changes these counts: judge it or
    // skip it on purpose.
    assert_eq!((judged, skipped), (JUDGED, SKIPPED));
}

#[test]
fn sum_pool_outside_the_dsp_rule_adds_claims() {
    let mut l = ResourceLedger::new();
    let slots = key(Owner::Core, LIMITS);
    l.declare(Owner::Core, &sum(LIMITS, 10)).unwrap();
    l.claim(pkg(1), &[want(slots, 4)]).unwrap();
    l.claim(pkg(2), &[want(slots, 6)]).unwrap();
    // exactly full is fine, one more is not
    assert_eq!(
        l.claim(pkg(3), &[want(slots, 1)]),
        Err(ClaimError::Short(PoolShort {
            pool: slots,
            need: 1,
            free: 0
        }))
    );
    let mut rows = [PoolReport {
        pool: slots,
        limit: 0,
        used: 0,
        largest: 0,
    }; 1];
    assert_eq!(l.report(&mut rows), 1);
    assert_eq!((rows[0].used, rows[0].largest), (10, 0));
    l.release(pkg(1));
    l.claim(pkg(3), &[want(slots, 4)]).unwrap();
}

/// A pool that is over its limit already (the tests lower the limit under existing claims; no claim can do it) refuses a claim that adds to it, through the
/// pool itself or through a pool that draws from it, and lets a claim that adds nothing to it through.
#[test]
fn pool_over_its_limit_refuses_only_claims_that_add_to_it() {
    let mut l = ResourceLedger::new();
    let parent = key(Owner::Core, DSP);
    let other = key(Owner::Core, LIMITS);
    l.declare(Owner::Core, &sum(DSP, 10)).unwrap();
    l.declare(Owner::Core, &sum(LIMITS, 5)).unwrap();
    let child_decl = ResourceDecl {
        draws_from: Some(parent),
        ..sum(FX, 100)
    };
    l.declare(Owner::Package(pkg(1)), &child_decl).unwrap();
    let child = key(Owner::Package(pkg(1)), FX);
    l.claim(pkg(2), &[want(parent, 4)]).unwrap();
    l.claim(pkg(3), &[want(child, 4)]).unwrap();
    // the device now reports less capacity: the parent holds 8 against a limit of 6
    let slot = l.find(parent).unwrap();
    l.pools[slot].as_mut().unwrap().limit = 6;
    assert_eq!(
        l.fits(&[want(parent, 1)]),
        Err(PoolShort {
            pool: parent,
            need: 1,
            free: 0
        })
    );
    // through the child: the parent is named, with nothing free
    assert_eq!(
        l.claim(pkg(4), &[want(child, 1)]),
        Err(ClaimError::Short(PoolShort {
            pool: parent,
            need: 1,
            free: 0
        }))
    );
    // a claim that adds nothing to the parent is not blamed for the overflow
    l.claim(pkg(4), &[want(other, 5)]).unwrap();
    l.claim(pkg(4), &[want(child, 0)]).unwrap();
}

#[test]
fn full_claims_table_is_not_a_pool_shortage() {
    let mut l = ResourceLedger::new();
    let pools: Vec<PoolKey> = (0..MAX_POOLS as u32)
        .map(|n| {
            l.declare(Owner::Core, &sum(100 + n, u32::MAX)).unwrap();
            key(Owner::Core, 100 + n)
        })
        .collect();
    // the bound: every package of the loader claims every pool
    let every_pool: Vec<Want> = pools.iter().map(|&p| want(p, 1)).collect();
    for n in 0..MAX_CONSUMERS as u8 {
        l.claim(pkg(n), &every_pool).unwrap();
    }
    // past it the table is full: a distinct error, and nothing changes
    assert_eq!(
        l.claim(pkg(MAX_CONSUMERS as u8), &every_pool),
        Err(ClaimError::Full)
    );
    assert_eq!(
        l.claim(pkg(MAX_CONSUMERS as u8), &[want(pools[0], 1)]),
        Err(ClaimError::Full)
    );
    // a consumer that holds rows adds to them without a new row
    l.claim(pkg(0), &[want(pools[0], 1)]).unwrap();
    l.release(pkg(0));
    l.claim(pkg(MAX_CONSUMERS as u8), &[want(pools[0], 1)])
        .unwrap();
}

#[test]
fn claims_table_rows_are_eight_bytes() {
    assert_eq!(core::mem::size_of::<Option<claim::Claim>>(), 8);
    assert_eq!(
        core::mem::size_of::<[Option<claim::Claim>; MAX_CLAIMS]>(),
        8 * MAX_CLAIMS
    );
}
