use super::*;

fn root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The schema of `body` (a proto3 file that imports dk/options.proto) or its error text.
fn read(body: &str) -> std::result::Result<Schema, String> {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("t.proto"),
        format!("syntax = \"proto3\";\npackage t;\nimport \"dk/options.proto\";\n{body}"),
    )
    .unwrap();
    Schema::read(&[dir.path(), &root().join("sdk/proto")], "t.proto").map_err(|e| e.0)
}

#[test]
fn the_example_schema_is_accepted_and_each_message_follows_the_messages_it_holds() {
    let wire = root().join("tests/fixtures/wire");
    let s = Schema::read(&[&wire, &root().join("sdk/proto")], "project_manager.proto").unwrap();
    let names: Vec<&str> = s.messages.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["ListRequest", "Project", "ListReply", "Coverage"]);
    assert!(s.messages[1].nested && !s.messages[0].nested);
}

#[test]
fn every_allowed_type_is_accepted() {
    let s = read(
        "enum E { E_ZERO = 0; }
        message In { uint32 a = 1; }
        message M {
          uint32 a = 1; int32 b = 2; bool c = 3; E d = 4; In e = 5;
          string f = 6 [(dk.max_len) = 3]; bytes g = 7 [(dk.max_len) = 3];
          repeated uint32 h = 8 [(dk.max_count) = 2]; repeated In i = 9 [(dk.max_count) = 2];
          repeated string j = 10 [(dk.max_len) = 3, (dk.max_count) = 2];
        }",
    );
    assert!(s.is_ok(), "{s:?}");
}

#[test]
fn what_the_subset_leaves_out_is_refused_with_the_field_named() {
    let cases = [
        ("message M { map<string, uint32> a = 1; }", "t.M.a: a map"),
        ("message M { oneof o { uint32 a = 1; } }", "t.M.a: oneof"),
        ("message M { optional uint32 a = 1; }", "t.M.a: oneof"),
        ("message M { float a = 1; }", "t.M.a: type float"),
        ("message M { double a = 1; }", "t.M.a: type double"),
        ("message M { uint64 a = 1; }", "t.M.a: type uint64"),
        ("message M { sint32 a = 1; }", "t.M.a: type sint32"),
        ("message M { fixed32 a = 1; }", "t.M.a: type fixed32"),
        (
            "message M { string a = 1; }",
            "t.M.a: a string or bytes field needs (dk.max_len)",
        ),
        (
            "message M { bytes a = 1; }",
            "t.M.a: a string or bytes field needs (dk.max_len)",
        ),
        (
            "message M { repeated uint32 a = 1; }",
            "t.M.a: a repeated field needs (dk.max_count)",
        ),
        (
            "message M { repeated bytes a = 1 [(dk.max_len) = 2, (dk.max_count) = 2]; }",
            "t.M.a: repeated bytes",
        ),
        (
            "message M { uint32 a = 1 [(dk.max_len) = 4]; }",
            "t.M.a: (dk.max_len) is for string and bytes only",
        ),
        (
            "message M { uint32 a = 1 [(dk.max_count) = 4]; }",
            "t.M.a: (dk.max_count) is for repeated fields only",
        ),
        (
            "message M { string a = 1 [(dk.max_len) = 0]; }",
            "t.M.a: dk.max_len must be at least 1",
        ),
        (
            "message M { repeated uint32 a = 1 [packed = false, (dk.max_count) = 2]; }",
            "t.M.a: a repeated scalar must be packed",
        ),
        ("message M { M a = 1; }", "contains itself"),
        (
            "message A { B b = 1; } message B { A a = 1; }",
            "contains itself",
        ),
        (
            "message M { uint32 a_len = 1; string a = 2 [(dk.max_len) = 1]; }",
            "member name a_len is used twice",
        ),
    ];
    for (body, want) in cases {
        let got = read(body).expect_err(body);
        assert!(got.contains(want), "{body}: {got}");
    }
}

#[test]
fn proto2_and_a_schema_without_the_options_import_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("p2.proto"),
        "syntax = \"proto2\";\nimport \"dk/options.proto\";\nmessage M { optional uint32 a = 1; }",
    )
    .unwrap();
    let got = Schema::read(&[dir.path(), &root().join("sdk/proto")], "p2.proto").unwrap_err();
    assert!(got.0.contains("only proto3"), "{got}");
    std::fs::write(
        dir.path().join("p3.proto"),
        "syntax = \"proto3\";\nmessage M { uint32 a = 1; }",
    )
    .unwrap();
    let got = Schema::read(&[dir.path(), &root().join("sdk/proto")], "p3.proto").unwrap_err();
    assert!(got.0.contains("must import dk/options.proto"), "{got}");
}
