use super::*;
use std::path::Path;

fn generated() -> Generated {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let schema = Schema::read(
        &[&root.join("tests/fixtures/wire"), &root.join("sdk/proto")],
        "project_manager.proto",
    )
    .unwrap();
    CCodec::render(&schema, "pm", "pm.h")
}

#[test]
fn the_largest_encoding_is_computed_from_the_bounds() {
    let h = generated().header;
    // key + 5 (uint32), key + length + 16 (name), key + 1 (bool)
    assert!(h.contains("#define pm_Project_MAX_SIZE 26u"));
    // 16 items of key + length + 26, and `next`
    assert!(h.contains("#define pm_ListReply_MAX_SIZE 454u"));
    assert!(h.contains("#define pm_ListRequest_MAX_SIZE 6u"));
}

#[test]
fn the_header_calls_no_libc_and_has_a_size_only_for_a_held_message() {
    let g = generated();
    for call in ["memcpy", "memset", "strlen", "malloc"] {
        assert!(
            !g.header.contains(call) && !g.source.contains(call),
            "{call}"
        );
    }
    assert!(g.source.contains("pm_Project_size") && !g.source.contains("pm_ListReply_size"));
    assert!(g.header.contains("#define PM_MODE_HARD 2"));
}
