use super::*;
use crate::build::{
    BuildOptions, Status, build_module, load_object, manifest, module_status, object_files,
    toolchain::Env,
};

const DECLARATION: &str = r#"
[workbench]
version = 1
component = "workbench/Panel.tsx"

[[setting]]
id = "boot"
label = "État"
type = "text"
max_bytes = 16

[[setting]]
id = "active"
label = "Animation"
type = "file-name"
max_bytes = 47
apply = "next-restart"
extension = "dkbs"

[[operation]]
id = "readAtInit"
output = "optional-text"
"#;

#[test]
fn the_device_descriptor_contains_the_display_name_and_public_fields() {
    let out = encode(DECLARATION, "test/module", "Démarrage").unwrap();
    assert_eq!(
        out,
        b"\x01\x0aD\xc3\xa9marrage\x02\x04boot\x05\xc3\x89tat\x10\x00\x00\
          \x06active\x09Animation\x2f\x01\x01\x04dkbs\x04None"
    );
}

#[test]
fn invalid_declarations_are_refused() {
    for (old, new, reason) in [
        ("version = 1", "version = 2", "version"),
        ("\"boot\"", "\"a/b\"", "settings key"),
        ("\"boot\"", "\"..\"", "settings key"),
        ("\"boot\"", "\"été\"", "settings key"),
        ("\"boot\"", "\"\"", "settings key"),
        ("\"État\"", "\"\"", "settings label"),
        ("\"text\"", "\"integer\"", "integer"),
        ("max_bytes = 16", "max_bytes = 0", "max_bytes"),
        ("max_bytes = 16", "max_bytes = 65", "max_bytes"),
        ("max_bytes = 47", "max_bytes = 48", "max_bytes"),
        ("max_bytes = 47", "max_bytes = 5", "max_bytes"),
        ("next-restart", "later", "later"),
        ("extension = \"dkbs\"", "", "extension"),
        ("dkbs", "", "extension"),
        ("dkbs", "DKBS", "extension"),
        ("dkbs", ".dkbs", "extension"),
        ("dkbs", "d/b", "extension"),
        ("dkbs", "é", "extension"),
        ("dkbs", "abcdefghijklmnopq", "extension"),
        (
            "max_bytes = 16",
            "max_bytes = 16\nextension = \"txt\"",
            "require type file-name",
        ),
        ("\"optional-text\"", "\"boolean\"", "boolean"),
        ("\"readAtInit\"", "\"boot\"", "duplicate"),
    ] {
        let source = DECLARATION.replace(old, new);
        let error = encode(&source, "test/module", "Test").unwrap_err();
        assert!(error.0.contains(reason), "{source}\n{error}");
    }
    let duplicate = format!(
        "{DECLARATION}\n[[setting]]\nid = \"boot\"\nlabel = \"Another\"\ntype = \"text\"\nmax_bytes = 32\n"
    );
    assert!(
        encode(&duplicate, "test/module", "Test")
            .unwrap_err()
            .0
            .contains("duplicate")
    );
    assert!(encode("[workbench]\nversion = 1", "test/module", "Test").is_err());
    assert!(encode(DECLARATION, "test/module", &"é".repeat(128)).is_err());
    assert!(encode(DECLARATION, &"m".repeat(KEY_MAX), "Test").is_err());
}

#[test]
fn the_descriptor_allows_exactly_512_encoded_bytes() {
    let source = DECLARATION.replace("État", &"x".repeat(220));
    assert_eq!(
        encode(&source, "test/module", &"x".repeat(250))
            .unwrap()
            .len(),
        512
    );
    assert!(
        encode(&source, "test/module", &"x".repeat(251))
            .unwrap_err()
            .0
            .contains("exceeds 512")
    );
}

#[test]
fn the_coldfire_cache_tracks_an_external_modules_workbench_source_and_name() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let env = Env::new(&root);
    if !env.m68k_or_skip() {
        return;
    }
    let dir = TempDir::new("pk-workbench-test-").unwrap();
    let module = dir.path().join("module.toml");
    let workbench = dir.path().join("workbench.toml");
    let text = format!(
        r#"
[module]
name = "A label"
id = "test/{}"
category = "test"

[[object]]
path = "build/test.dko.json"
sources = ["module.c"]
"#,
        std::process::id()
    );
    std::fs::write(&module, &text).unwrap();
    std::fs::write(&workbench, DECLARATION).unwrap();
    std::fs::write(dir.path().join("module.c"), "#include \"workbench_descriptor.h\"\nconst uint8_t *descriptor(void) { return dk_workbench_descriptor; }\n").unwrap();
    let object_path = || object_files(&root, dir.path()).unwrap().remove(0).1;
    let before = object_path();
    assert_eq!(object_path(), before);
    let opts = BuildOptions {
        root: root.clone(),
        ..Default::default()
    };
    let built = build_module(dir.path(), &opts).unwrap().objects.remove(0);
    assert_eq!(built.path, before);
    assert_eq!(
        module_status(&root, dir.path()).unwrap(),
        [("build/test.dko.json".into(), true)]
    );
    assert_eq!(
        build_module(dir.path(), &opts).unwrap().objects[0].status,
        Status::Unchanged
    );

    std::fs::write(&workbench, DECLARATION.replace("next-restart", "immediate")).unwrap();
    let modified = object_path();
    assert_ne!(modified, before);
    assert!(!modified.exists());
    let stored = load_object(
        &env,
        dir.path(),
        &manifest(dir.path()).unwrap(),
        "build/test.dko.json",
    )
    .unwrap();
    assert_eq!(std::fs::read_to_string(&modified).unwrap(), stored);
    std::fs::write(&module, text.replace("A label", "Another label")).unwrap();
    assert_ne!(object_path(), modified);
    std::fs::remove_file(&workbench).unwrap();
    assert!(object_files(&root, dir.path()).is_err());
}
