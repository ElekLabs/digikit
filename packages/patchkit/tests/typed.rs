//! Generated module files and the manifest checks over the real device models (skipped when the repository has no toolchain).

use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// An `[[fx]]` table takes an id only; the old fields (`process`, `desc`) are refused, so an effect is never dropped without a word.
#[test]
fn a_manifest_fx_section_is_refused() {
    let dir = root().join("tests/fixtures/modules/dyn-fx-gain");
    let mut text = std::fs::read_to_string(dir.join("module.toml")).unwrap();
    text.push_str("\n[[fx]]\nid = \"gain\"\nprocess = \"_dg_process\"\ndesc = \"_dg_desc\"\n");
    let none = |p: &str| Err(patchkit::Error(format!("{p}: no such file")));
    let e = patchkit::module::Module::from_parts_lenient(&text, &none).unwrap_err();
    assert!(e.to_string().contains("unknown field"), "{e}");
}

/// A manifest that still declares params, pages or voices is refused: a module declares them in `init` (`[[machine]]` takes an id only).
#[test]
fn a_manifest_machine_section_is_refused() {
    let dir = root().join("tests/fixtures/modules/dyn-dn2");
    let base = std::fs::read_to_string(dir.join("module.toml")).unwrap();
    let none = |p: &str| Err(patchkit::Error(format!("{p}: no such file")));
    for section in ["param", "page", "voice"] {
        let text = format!("{base}\n[[{section}]]\nid = \"x\"\n");
        let e = patchkit::module::Module::from_parts_lenient(&text, &none).unwrap_err();
        assert!(
            e.to_string().contains("dk_module_ctx_add_machine") && e.to_string().contains(section),
            "{e}"
        );
    }
}
