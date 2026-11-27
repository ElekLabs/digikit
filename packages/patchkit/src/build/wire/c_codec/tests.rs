use super::*;
use std::path::Path;

fn header() -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let schema = Schema::read(
        &[&root.join("tests/fixtures/wire"), &root.join("sdk/proto")],
        "project_manager.proto",
    )
    .unwrap();
    CCodec::render(&schema, "pm")
}

#[test]
fn the_largest_encoding_is_computed_from_the_bounds() {
    let h = header();
    // key + 5 (uint32), key + length + 16 (name), key + 1 (bool)
    assert!(h.contains("#define pm_Project_MAX_SIZE 26u"));
    // 16 items of key + length + 26, and `next`
    assert!(h.contains("#define pm_ListReply_MAX_SIZE 454u"));
    assert!(h.contains("#define pm_ListRequest_MAX_SIZE 6u"));
}

#[test]
fn the_header_calls_no_libc_and_has_a_size_only_for_a_held_message() {
    let h = header();
    for call in ["memcpy", "memset", "strlen", "malloc"] {
        assert!(!h.contains(call), "{call}");
    }
    assert!(h.contains("pm_Project_size") && !h.contains("pm_ListReply_size"));
    assert!(h.contains("#define PM_MODE_HARD 2"));
}
