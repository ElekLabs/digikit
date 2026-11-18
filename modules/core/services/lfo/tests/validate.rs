//! Host tests of the shape-set rule (`Lfo::validate_records`): one case per refusal, and the fixture's record.
use dkrt_abi::service::{Problem, Service};
use dkrt_sdk::records::KIND_LFO_SHAPE;
use dkrt_svc_lfo::records::{Lfo, MAX_SHAPES};
use dkrt_svc_lfo::validate::{body_ok, what, BODY_LEN};
use dkrt_test_support::FakeCore;

type Body = [u8; BODY_LEN];

fn problems(set: &[Body]) -> Vec<Problem> {
    let refs: Vec<&[u8]> = set.iter().map(|b| &b[..]).collect();
    let mut out = Vec::new();
    <Lfo as Service<FakeCore>>::validate_records(KIND_LFO_SHAPE, &refs, &mut |p| out.push(p));
    out
}

fn shape(key: u32, name: &[u8; 4], f: impl Fn(i32) -> i32) -> Body {
    let mut b = [0u8; BODY_LEN];
    b[0..4].copy_from_slice(&key.to_be_bytes());
    b[4..8].copy_from_slice(name);
    for i in 0..32 {
        b[8 + 2 * i..10 + 2 * i].copy_from_slice(&(f(i as i32) as i16).to_be_bytes());
    }
    b
}

/// The ramp of `tests/fixtures/modules/lfo-shape`: from -31744 in steps of 2048.
fn ramp(key: u32, name: &[u8; 4]) -> Body {
    shape(key, name, |i| -31744 + 2048 * i)
}

fn whats(set: &[Body]) -> Vec<&'static str> {
    problems(set).iter().map(|p| p.what).collect()
}

#[test]
fn the_fixture_record_and_a_full_set_pass() {
    assert!(problems(&[ramp(0x1234_5600, b"RAMP")]).is_empty());
    let set: Vec<_> = (0..MAX_SHAPES as u32)
        .map(|k| ramp(0x100 * (k + 1), &[b'A' + k as u8, b'B', b'C', b'D']))
        .collect();
    assert!(problems(&set).is_empty());
}

#[test]
fn a_module_has_one_to_eight_shapes() {
    assert_eq!(whats(&[]), [what::COUNT]);
    let set: Vec<_> = (0..MAX_SHAPES as u32 + 1)
        .map(|k| ramp(0x100 * (k + 1), &[b'A' + k as u8, b'B', b'C', b'D']))
        .collect();
    assert_eq!(whats(&set), [what::COUNT]);
}

#[test]
fn a_name_has_four_printable_characters_and_the_key_is_not_zero() {
    assert_eq!(whats(&[ramp(0x100, b"AB\x01D")]), [what::RECORD]);
    assert_eq!(whats(&[ramp(0x100, b"AB\x7fD")]), [what::RECORD]);
    assert_eq!(whats(&[ramp(0, b"RAMP")]), [what::RECORD]);
    assert!(body_ok(&ramp(0x100, b"  ~ ")));
    assert!(!body_ok(&[0u8; 8]), "a short body is refused, not a panic");
}

#[test]
fn names_differ_and_differ_in_their_first_three_characters() {
    let a = ramp(0x100, b"SINE");
    assert_eq!(whats(&[a, ramp(0x200, b"SINE")]), [what::NAME_TWICE]);
    assert_eq!(whats(&[a, ramp(0x200, b"SINF")]), [what::SHORT_NAME_TWICE]);
    assert!(problems(&[a, ramp(0x200, b"TINE")]).is_empty());
    let p = problems(&[a, ramp(0x200, b"SINE")])[0];
    assert_eq!((p.index, p.with), (Some(1), Some(0)));
}

#[test]
fn a_key_is_used_once() {
    let p = problems(&[ramp(0x100, b"AAAA"), ramp(0x100, b"BBBB")]);
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].what, what::KEY_TWICE);
    assert_eq!(p[0].code, dkrt_sdk::E_TYPE_USED);
}

#[test]
fn a_flat_or_all_zero_shape_is_refused() {
    assert_eq!(whats(&[shape(0x100, b"FLAT", |_| 1000)]), [what::FLAT]);
    assert_eq!(whats(&[shape(0x100, b"ZERO", |_| 0)]), [what::ZERO]);
    // one sample that differs is enough
    assert!(problems(&[shape(0x100, b"BLIP", |i| i32::from(i == 5))]).is_empty());
}

#[test]
fn a_kind_the_service_does_not_own_is_accepted() {
    let mut out = Vec::new();
    <Lfo as Service<FakeCore>>::validate_records(0, &[], &mut |p| out.push(p));
    assert!(out.is_empty());
}

#[test]
fn problems_read_as_sentences() {
    let p = problems(&[ramp(0x100, b"SINE"), ramp(0x200, b"SINF")])[0];
    assert_eq!(
        p.to_string(),
        format!(
            "record kind {KIND_LFO_SHAPE}, record 1: {} (record 0)",
            what::SHORT_NAME_TWICE
        )
    );
}
