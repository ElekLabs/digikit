use super::*;
use crate::package::module_ui::vectors;
use std::vec;
use std::vec::Vec;

const MODULE: &[u8] = b"test/m";

/// A descriptor of one field `k` with the given kind tag, stored length and body.
fn one(version: u8, tag: u8, max: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![version, 1, b'N', 1, 1, b'k', 1, b'L', max, tag, 0];
    out.extend_from_slice(body);
    out
}

fn integer(min: i32, max: i32, step: u32, decimals: u8, default: i32, unit: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&min.to_be_bytes());
    body.extend_from_slice(&max.to_be_bytes());
    body.extend_from_slice(&step.to_be_bytes());
    body.push(decimals);
    body.extend_from_slice(&default.to_be_bytes());
    body.push(unit.len() as u8);
    body.extend_from_slice(unit);
    one(2, 2, 4, &body)
}

fn choice(labels: &[&[u8]], default: u8) -> Vec<u8> {
    let mut body = vec![labels.len() as u8, default];
    for label in labels {
        body.push(label.len() as u8);
        body.extend_from_slice(label);
    }
    one(2, 4, 1, &body)
}

fn parse(bytes: &[u8]) -> Option<Descriptor> {
    Descriptor::parse(MODULE, bytes)
}

fn accepts(descriptor: &[u8], value: &[u8]) -> bool {
    let parsed = parse(descriptor).unwrap();
    parsed.field(0).unwrap().accepts(parsed.bytes(), value)
}

#[test]
fn the_shared_vectors_parse_and_decide_stored_values() {
    let descriptors = vectors::descriptors();
    assert!(!descriptors.is_empty());
    for vector in &descriptors {
        let parsed = Descriptor::parse(vector.module.as_bytes(), &vector.bytes)
            .unwrap_or_else(|| panic!("{} parses", vector.name));
        assert_eq!(parsed.bytes(), vector.bytes.as_slice(), "{}", vector.name);
        for size in 0..vector.bytes.len() {
            assert!(
                Descriptor::parse(vector.module.as_bytes(), &vector.bytes[..size]).is_none(),
                "{} cut to {size}",
                vector.name
            );
        }
        let mut longer = vector.bytes.clone();
        longer.push(0);
        assert!(Descriptor::parse(vector.module.as_bytes(), &longer).is_none());
    }
    for value in vectors::values() {
        let vector = descriptors
            .iter()
            .find(|d| d.name == value.descriptor)
            .unwrap();
        let parsed = Descriptor::parse(vector.module.as_bytes(), &vector.bytes).unwrap();
        let index = parsed
            .key_index(value.setting.as_bytes())
            .unwrap_or_else(|| panic!("{} has {}", value.descriptor, value.setting));
        let field = parsed.field(index as u8).unwrap();
        assert_eq!(
            field.accepts(parsed.bytes(), &value.bytes),
            value.valid,
            "{}: {}",
            value.setting,
            value.rule
        );
    }
}

#[test]
fn the_first_byte_is_the_version_that_holds_every_kind() {
    let body = [1, 0];
    assert!(parse(&one(2, 3, 1, &body[..1])).is_some());
    assert!(
        parse(&one(1, 3, 1, &body[..1])).is_none(),
        "boolean in version 1"
    );
    assert!(parse(&integer(0, 10, 1, 0, 0, b"")).is_some());
    let mut v1 = integer(0, 10, 1, 0, 0, b"");
    v1[0] = 1;
    assert!(parse(&v1).is_none(), "integer in version 1");
    let mut v1 = choice(&[b"a"], 0);
    v1[0] = 1;
    assert!(parse(&v1).is_none(), "choice in version 1");
    assert!(parse(&one(2, 0, 4, &[])).is_some(), "text in version 2");
    assert!(parse(&one(3, 0, 4, &[])).is_none());
    assert!(parse(&one(0, 0, 4, &[])).is_none());
    assert!(parse(&one(2, 5, 1, &[])).is_none(), "unknown kind");
}

#[test]
fn an_integer_declares_its_range_step_and_default() {
    assert!(parse(&integer(0, 10, 5, 4, 5, b"dB")).is_some());
    assert!(parse(&integer(0, 10, 0, 0, 0, b"")).is_none(), "step 0");
    assert!(
        parse(&integer(5, 5, 1, 0, 5, b"")).is_none(),
        "min equals max"
    );
    assert!(
        parse(&integer(6, 5, 1, 0, 5, b"")).is_none(),
        "min above max"
    );
    assert!(
        parse(&integer(0, 11, 5, 0, 0, b"")).is_none(),
        "step does not divide"
    );
    assert!(parse(&integer(0, 10, 5, 5, 0, b"")).is_none(), "decimals 5");
    assert!(
        parse(&integer(0, 10, 5, 0, 11, b"")).is_none(),
        "default above max"
    );
    assert!(
        parse(&integer(0, 10, 5, 0, -5, b"")).is_none(),
        "default below min"
    );
    assert!(
        parse(&integer(0, 10, 5, 0, 3, b"")).is_none(),
        "default off the step"
    );
    assert!(
        parse(&integer(0, 10, 5, 0, 0, b"\xff")).is_none(),
        "unit is not UTF-8"
    );
    let mut wrong_max = integer(0, 10, 1, 0, 0, b"");
    wrong_max[8] = 5;
    assert!(parse(&wrong_max).is_none(), "stored length 5");
    // the largest span: every bound an i32 allows, in i64 arithmetic
    assert!(parse(&integer(i32::MIN, i32::MAX, 1, 0, 0, b"")).is_some());
    assert!(parse(&integer(i32::MIN, i32::MAX, u32::MAX, 0, i32::MIN, b"")).is_some());
    assert!(parse(&integer(i32::MIN, i32::MAX, 5, 0, 0, b"")).is_none());
    // 2^32 - 1 is a multiple of 3
    assert!(parse(&integer(i32::MIN, i32::MAX, 3, 0, i32::MIN, b"")).is_some());
}

#[test]
fn an_integer_checks_range_and_step_from_the_stored_bytes() {
    let stepped = integer(-10, 10, 5, 0, 0, b"");
    for (value, valid) in [
        (-10, true),
        (-5, true),
        (0, true),
        (10, true),
        (-15, false),
        (15, false),
        (1, false),
        (-9, false),
    ] {
        assert_eq!(
            accepts(&stepped, &i32::to_be_bytes(value)),
            valid,
            "{value}"
        );
    }
    assert!(!accepts(&stepped, &[0, 0, 0]));
    assert!(!accepts(&stepped, &[0, 0, 0, 0, 0]));
    assert!(!accepts(&stepped, &[]));
    let wide = integer(i32::MIN, i32::MAX, 1, 0, 0, b"");
    assert!(accepts(&wide, &i32::MIN.to_be_bytes()));
    assert!(accepts(&wide, &i32::MAX.to_be_bytes()));
    let top = integer(i32::MIN, i32::MAX, u32::MAX, 0, i32::MIN, b"");
    assert!(accepts(&top, &i32::MIN.to_be_bytes()));
    assert!(accepts(&top, &i32::MAX.to_be_bytes()));
    assert!(!accepts(&top, &0i32.to_be_bytes()));
}

#[test]
fn a_boolean_is_one_byte_zero_or_one() {
    let boolean = one(2, 3, 1, &[1]);
    assert!(accepts(&boolean, &[0]) && accepts(&boolean, &[1]));
    assert!(!accepts(&boolean, &[2]) && !accepts(&boolean, &[0, 0]) && !accepts(&boolean, &[]));
    assert!(parse(&one(2, 3, 1, &[2])).is_none(), "default 2");
    assert!(parse(&one(2, 3, 2, &[1])).is_none(), "stored length 2");
    assert!(parse(&one(2, 3, 1, &[])).is_none(), "no default");
    assert!(parse(&one(2, 3, 1, &[1, 0])).is_none(), "trailing byte");
}

#[test]
fn a_choice_holds_one_to_thirty_two_labels() {
    let label: &[u8] = b"x";
    let labels = vec![label; 33];
    assert!(parse(&choice(&labels[..32], 31)).is_some());
    assert!(parse(&choice(&labels[..33], 0)).is_none(), "33 labels");
    assert!(parse(&choice(&[], 0)).is_none(), "no labels");
    assert!(
        parse(&choice(&[b"a", b"b"], 2)).is_none(),
        "default is the count"
    );
    assert!(parse(&choice(&[b"a", b""], 0)).is_none(), "empty label");
    assert!(
        parse(&choice(&[b"\xff"], 0)).is_none(),
        "label is not UTF-8"
    );
    let mut wrong_max = choice(&[b"a"], 0);
    wrong_max[8] = 2;
    assert!(parse(&wrong_max).is_none(), "stored length 2");

    let three = choice(&[b"a", b"b", b"c"], 1);
    assert!(accepts(&three, &[0]) && accepts(&three, &[2]));
    assert!(!accepts(&three, &[3]) && !accepts(&three, &[0, 0]) && !accepts(&three, &[]));
    let many = choice(&labels[..32], 0);
    assert!(accepts(&many, &[31]) && !accepts(&many, &[32]));
}

#[test]
fn a_descriptor_may_fill_all_512_bytes() {
    let label: &[u8] = &[b'a'; 14];
    let last: &[u8] = &[b'b'; 33];
    let mut labels = vec![label; 31];
    labels.push(last);
    let full = choice(&labels, 0);
    assert_eq!(full.len(), DESCRIPTOR_MAX);
    assert!(parse(&full).is_some());
    labels[31] = &[b'b'; 34];
    let over = choice(&labels, 0);
    assert_eq!(over.len(), DESCRIPTOR_MAX + 1);
    assert!(parse(&over).is_none());
}

#[test]
fn text_and_file_name_keep_their_checks() {
    assert!(accepts(&one(1, 0, 4, &[]), b"abcd"));
    assert!(!accepts(&one(1, 0, 4, &[]), b"abcde"));
    assert!(!accepts(&one(1, 0, 4, &[]), b"\xff"));
    assert!(parse(&one(1, 0, 65, &[])).is_none());
    let file = one(1, 1, 20, b"\x03abc\x04None");
    assert!(accepts(&file, b"a.abc") && accepts(&file, b"A.ABC"));
    assert!(!accepts(&file, b"a.txt") && !accepts(&file, b".abc") && !accepts(&file, b"a/b.abc"));
    assert_eq!(
        parse(&file).unwrap().field(0).unwrap().extension(&file),
        b"abc"
    );
    assert!(
        parse(&one(1, 1, 4, b"\x03abc\x04None")).is_none(),
        "no room for a stem"
    );
}
