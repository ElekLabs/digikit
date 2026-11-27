use super::*;

fn decl(toml: &str) -> ServiceDecl {
    toml::from_str(toml).unwrap()
}

fn check(toml: &str) -> Result<()> {
    decl(toml).validate("test/svc", &|k| k == "fx_slots")
}

fn refused(toml: &str, needle: &str) {
    let e = check(toml).unwrap_err().to_string();
    assert!(e.contains(needle), "{e:?} lacks {needle:?}");
}

#[test]
fn the_plan_example_is_accepted() {
    check(
        r#"
[[resource]]
name = "effect_slots"
limit = "profile:fx_slots"
[[resource]]
name = "fx_cost"
draws_from = "core.dsp_cost"
aggregate = "sum"
[[resource]]
name = "voice_cost"
aggregate = "max"
scale = 8
draws_from = "fx_cost"
[[resource]]
name = "late"
limit = "init"
[[record]]
name = "effect"
kind = 1
len = 0x1a4
"#,
    )
    .unwrap();
}

#[test]
fn names_and_kinds_are_unique() {
    refused(
        "[[resource]]\nname = \"a\"\n[[resource]]\nname = \"a\"\n",
        "declared twice",
    );
    refused(
        "[[record]]\nname = \"a\"\nkind = 1\nlen = 4\n[[record]]\nname = \"a\"\nkind = 2\nlen = 4\n",
        "declared twice",
    );
    refused(
        "[[record]]\nname = \"a\"\nkind = 1\nlen = 4\n[[record]]\nname = \"b\"\nkind = 1\nlen = 4\n",
        "kind 1 is used twice",
    );
}

#[test]
fn a_kind_is_sixteen_bits() {
    assert!(
        toml::from_str::<ServiceDecl>("[[record]]\nname = \"a\"\nkind = 65536\nlen = 4\n").is_err()
    );
}

#[test]
fn profile_keys_must_exist() {
    refused(
        "[[resource]]\nname = \"a\"\nlimit = \"profile:nope\"\n",
        "no integer `nope`",
    );
    refused(
        "[[resource]]\nname = \"a\"\naggregate = \"max\"\nscale = \"profile:nope\"\n",
        "no integer `nope`",
    );
    refused(
        "[[resource]]\nname = \"a\"\nlimit = \"lots\"\n",
        "\"profile:<key>\" or \"init\"",
    );
}

#[test]
fn draws_from_resolves() {
    refused(
        "[[resource]]\nname = \"a\"\ndraws_from = \"core.nothing\"\n",
        "draws_from",
    );
    refused(
        "[[resource]]\nname = \"a\"\ndraws_from = \"b\"\n",
        "draws_from",
    );
    refused(
        "[[resource]]\nname = \"a\"\ndraws_from = \"a\"\n",
        "draws_from",
    );
}

#[test]
fn scale_belongs_to_max() {
    refused(
        "[[resource]]\nname = \"a\"\nscale = 2\n",
        "`scale` belongs to",
    );
    refused(
        "[[resource]]\nname = \"a\"\naggregate = \"sum\"\nscale = 2\n",
        "`scale` belongs to",
    );
}

#[test]
fn unknown_fields_and_aggregates_are_refused() {
    assert!(toml::from_str::<ServiceDecl>("[[resource]]\nname = \"a\"\nlimt = 1\n").is_err());
    assert!(
        toml::from_str::<ServiceDecl>("[[resource]]\nname = \"a\"\naggregate = \"min\"\n").is_err()
    );
}
