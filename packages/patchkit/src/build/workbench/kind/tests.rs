use super::*;
use crate::build::workbench::fixture::{vector_descriptors, vector_values};
use crate::build::workbench::schema::Schema;

fn parse(fields: &str) -> Result<Kind> {
    let raw: RawField = toml::from_str(&format!("id = \"x\"\nlabel = \"X\"\n{fields}")).unwrap();
    Kind::parse(&raw)
}

fn refused(fields: &str, reason: &str) {
    let error = parse(fields).unwrap_err();
    assert!(error.0.contains(reason), "{fields}\n{error}");
}

const INTEGER: &str = "type = \"integer\"\nmin = 0\nmax = 100\ndefault = 50\n";

#[test]
fn an_integer_takes_defaults_for_step_decimals_and_unit() {
    assert_eq!(
        parse(INTEGER).unwrap(),
        Kind::Integer {
            min: 0,
            max: 100,
            step: 1,
            decimals: 0,
            unit: String::new(),
            default: 50
        }
    );
    assert!(parse(&format!("{INTEGER}step = 5\ndecimals = 4\nunit = \"dB\"")).is_ok());
}

#[test]
fn an_integer_accepts_the_extremes_of_i32() {
    let kind =
        parse("type = \"integer\"\nmin = -2147483648\nmax = 2147483647\ndefault = -2147483648")
            .unwrap();
    assert!(kind.accepts(&i32::MIN.to_be_bytes()));
    assert!(kind.accepts(&i32::MAX.to_be_bytes()));
    // a step that divides the whole span, which is larger than i32::MAX
    assert!(
        parse("type = \"integer\"\nmin = -2147483648\nmax = 2147483647\nstep = 4294967295\ndefault = -2147483648")
            .is_ok()
    );
    refused(
        "type = \"integer\"\nmin = -2147483648\nmax = 2147483647\nstep = 2\ndefault = 0",
        "multiple of step",
    );
}

#[test]
fn an_integer_rule_has_a_refusal_for_each_edge() {
    for (fields, reason) in [
        ("type = \"integer\"\nmax = 100\ndefault = 5", "min and max"),
        ("type = \"integer\"\nmin = 0\ndefault = 5", "min and max"),
        (
            "type = \"integer\"\nmin = 5\nmax = 5\ndefault = 5",
            "greater than min",
        ),
        (
            "type = \"integer\"\nmin = 5\nmax = 4\ndefault = 5",
            "greater than min",
        ),
        ("type = \"integer\"\nmin = 0\nmax = 100", "integer default"),
        (
            "type = \"integer\"\nmin = 0\nmax = 100\ndefault = \"5\"",
            "integer default",
        ),
        (
            "type = \"integer\"\nmin = 0\nmax = 100\ndefault = 101",
            "outside min and max or not on a step",
        ),
        (
            "type = \"integer\"\nmin = 0\nmax = 100\ndefault = -1",
            "outside min and max or not on a step",
        ),
        (
            "type = \"integer\"\nmin = 0\nmax = 100\ndefault = 4294967296",
            "outside min and max or not on a step",
        ),
        (
            "type = \"integer\"\nmin = 0\nmax = 100\nstep = 0\ndefault = 0",
            "at least 1",
        ),
        (
            "type = \"integer\"\nmin = 0\nmax = 100\nstep = 7\ndefault = 0",
            "multiple of step",
        ),
        (
            "type = \"integer\"\nmin = 0\nmax = 100\nstep = 5\ndefault = 3",
            "outside min and max or not on a step",
        ),
        (
            "type = \"integer\"\nmin = 0\nmax = 100\ndecimals = 5\ndefault = 0",
            "decimals",
        ),
        (
            "type = \"integer\"\nmin = 0\nmax = 100\nunit = \"123456789\"\ndefault = 0",
            "unit",
        ),
        (
            "type = \"integer\"\nmin = 0\nmax = 100\nunit = \"é\"\ndefault = 0",
            "unit",
        ),
        (
            "type = \"integer\"\nmin = 0\nmax = 100\nunit = \"a\\tb\"\ndefault = 0",
            "unit",
        ),
        (
            "type = \"integer\"\nmin = 0\nmax = 100\nmax_bytes = 4\ndefault = 0",
            "max_bytes does not apply",
        ),
    ] {
        refused(fields, reason);
    }
    assert!(
        parse("type = \"integer\"\nmin = 0\nmax = 100\nunit = \"12345678\"\ndefault = 0").is_ok()
    );
    assert!(parse("type = \"integer\"\nmin = 0\nmax = 100\ndecimals = 4\ndefault = 0").is_ok());
}

#[test]
fn a_boolean_needs_a_boolean_default_and_nothing_else() {
    assert_eq!(
        parse("type = \"boolean\"\ndefault = false").unwrap(),
        Kind::Boolean { default: false }
    );
    refused("type = \"boolean\"", "default");
    refused("type = \"boolean\"\ndefault = 1", "default");
    refused(
        "type = \"boolean\"\ndefault = true\nmax_bytes = 1",
        "max_bytes does not apply",
    );
    refused(
        "type = \"boolean\"\ndefault = true\nextension = \"a\"",
        "require type file-name",
    );
}

#[test]
fn a_choice_holds_one_to_32_unique_labels_and_a_default_among_them() {
    let labels = |n: usize| {
        (0..n)
            .map(|i| format!("\"L{i}\""))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let kind = parse(&format!(
        "type = \"choice\"\nchoices = [{}]\ndefault = \"L31\"",
        labels(32)
    ))
    .unwrap();
    assert!(matches!(&kind, Kind::Choice { labels, default: 31 } if labels.len() == 32));
    assert!(parse("type = \"choice\"\nchoices = [\"Only\"]\ndefault = \"Only\"").is_ok());
    for (fields, reason) in [
        (
            format!(
                "type = \"choice\"\nchoices = [{}]\ndefault = \"L0\"",
                labels(33)
            ),
            "1 to 32",
        ),
        (
            "type = \"choice\"\nchoices = []\ndefault = \"a\"".into(),
            "1 to 32",
        ),
        ("type = \"choice\"\ndefault = \"a\"".into(), "needs choices"),
        (
            "type = \"choice\"\nchoices = [\"a\"]".into(),
            "default label",
        ),
        (
            "type = \"choice\"\nchoices = [\"a\"]\ndefault = \"b\"".into(),
            "not one of",
        ),
        (
            "type = \"choice\"\nchoices = [\"a\"]\ndefault = 0".into(),
            "default label",
        ),
        (
            "type = \"choice\"\nchoices = [\"a\", \"a\"]\ndefault = \"a\"".into(),
            "duplicate",
        ),
        (
            "type = \"choice\"\nchoices = [\"\"]\ndefault = \"\"".into(),
            "1 to 24",
        ),
        (
            format!(
                "type = \"choice\"\nchoices = [\"{}\"]\ndefault = \"x\"",
                "x".repeat(25)
            ),
            "1 to 24",
        ),
        (
            "type = \"choice\"\nchoices = [\"--\"]\ndefault = \"--\"".into(),
            "letter or a digit",
        ),
        (
            "type = \"choice\"\nchoices = [\"a\"]\ndefault = \"a\"\nmax_bytes = 1".into(),
            "max_bytes does not apply",
        ),
    ] {
        refused(&fields, reason);
    }
    let longest = "x".repeat(24);
    assert!(
        parse(&format!(
            "type = \"choice\"\nchoices = [\"{longest}\"]\ndefault = \"{longest}\""
        ))
        .is_ok()
    );
}

#[test]
fn text_and_file_names_still_need_max_bytes_and_refuse_the_new_fields() {
    refused("type = \"text\"", "max_bytes is required");
    refused(
        "type = \"file-name\"\nextension = \"a\"",
        "max_bytes is required",
    );
    refused(
        "type = \"text\"\nmax_bytes = 4\ndefault = \"a\"",
        "default does not apply",
    );
    refused(
        "type = \"text\"\nmax_bytes = 4\nmin = 1",
        "min does not apply",
    );
    refused(
        "type = \"file-name\"\nmax_bytes = 9\nextension = \"a\"\nchoices = [\"a\"]",
        "choices does not apply",
    );
    assert!(parse("type = \"text\"\nmax_bytes = 64").is_ok());
    assert!(parse("type = \"file-name\"\nmax_bytes = 47\nextension = \"txt\"").is_ok());
}

#[test]
fn the_shared_vectors_agree_with_the_accept_rules() {
    let mut schemas = std::collections::HashMap::new();
    for d in vector_descriptors() {
        schemas.insert(
            d.name.clone(),
            Schema::parse(&d.toml, &d.module, &d.module_name).unwrap(),
        );
    }
    let values = vector_values();
    assert!(values.iter().any(|v| v.valid) && values.iter().any(|v| !v.valid));
    for v in values {
        let kind = schemas[&v.descriptor].setting(&v.setting).unwrap().kind();
        assert_eq!(
            kind.accepts(&v.value),
            v.valid,
            "{}: {} {:02x?}",
            v.descriptor,
            v.rule,
            v.value
        );
    }
}
