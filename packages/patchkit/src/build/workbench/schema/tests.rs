use super::*;
use crate::build::workbench::fixture::DECLARATION;

fn parse(source: &str) -> Result<Schema> {
    Schema::parse(source, "test/module", "Test")
}

fn setting(id: &str, rest: &str) -> String {
    format!("\n[[setting]]\nid = \"{id}\"\nlabel = \"L\"\n{rest}\n")
}

#[test]
fn invalid_declarations_are_refused() {
    for (old, new, reason) in [
        ("version = 1", "version = 2", "version"),
        ("\"boot\"", "\"a/b\"", "settings key"),
        ("\"boot\"", "\"..\"", "settings key"),
        ("\"boot\"", "\"été\"", "settings key"),
        ("\"boot\"", "\"\"", "settings key"),
        ("\"boot\"", "\"a.b\"", "settings key"),
        ("\"boot\"", "\"2fast\"", "settings key"),
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
        ("max_bytes = 16", "max_bytes = 16\nwidth = 3", "width"),
    ] {
        let source = DECLARATION.replace(old, new);
        let error = parse(&source).unwrap_err();
        assert!(error.0.contains(reason), "{source}\n{error}");
    }
    let duplicate = format!(
        "{DECLARATION}{}",
        setting("boot", "type = \"text\"\nmax_bytes = 32")
    );
    assert!(parse(&duplicate).unwrap_err().0.contains("duplicate"));
    assert!(parse("[workbench]\nversion = 1").is_err());
    assert!(Schema::parse(DECLARATION, "test/module", &"é".repeat(128)).is_err());
    assert!(Schema::parse(DECLARATION, "test/module", "").is_err());
    assert!(Schema::parse(DECLARATION, &"m".repeat(KEY_MAX_FOR_TEST), "Test").is_err());
}

const KEY_MAX_FOR_TEST: usize = dkrt_sdk::settings::KEY_MAX;

#[test]
fn names_that_meet_in_one_c_identifier_are_refused() {
    let head = "[workbench]\nversion = 1\n";
    // ids that map to the same C name
    let source = format!(
        "{head}{}{}",
        setting("a-b", "type = \"boolean\"\ndefault = true"),
        setting("a_b", "type = \"boolean\"\ndefault = true")
    );
    assert!(parse(&source).unwrap_err().0.contains("C name"));
    let source = format!(
        "{head}{}{}",
        setting("Mix", "type = \"boolean\"\ndefault = true"),
        setting("mix", "type = \"boolean\"\ndefault = true")
    );
    assert!(parse(&source).is_err());
    // choice labels that map to the same C name
    let source = format!(
        "{head}{}",
        setting(
            "m",
            "type = \"choice\"\nchoices = [\"a b\", \"a-b\"]\ndefault = \"a b\""
        )
    );
    assert!(parse(&source).unwrap_err().0.contains("C name"));
    // a label that meets a macro of the same setting, and one that meets the key macro of another
    let source = format!(
        "{head}{}",
        setting(
            "m",
            "type = \"choice\"\nchoices = [\"Off\", \"Default\"]\ndefault = \"Off\""
        )
    );
    assert!(
        parse(&source)
            .unwrap_err()
            .0
            .contains("WB_SETTING_M_DEFAULT")
    );
    let source = format!(
        "{head}{}{}",
        setting(
            "m",
            "type = \"choice\"\nchoices = [\"Off\", \"Key\"]\ndefault = \"Off\""
        ),
        setting("m_key", "type = \"boolean\"\ndefault = true")
    );
    assert!(parse(&source).unwrap_err().0.contains("WB_SETTING_M_KEY"));
    // distinct names are fine
    let source = format!(
        "{head}{}{}",
        setting(
            "m",
            "type = \"choice\"\nchoices = [\"Off\", \"On\"]\ndefault = \"Off\""
        ),
        setting("m2", "type = \"boolean\"\ndefault = true")
    );
    assert!(parse(&source).is_ok());
}

#[test]
fn the_version_is_1_until_a_setting_uses_a_new_type() {
    assert_eq!(parse(DECLARATION).unwrap().descriptor_version(), 1);
    for rest in [
        "type = \"integer\"\nmin = 0\nmax = 1\ndefault = 0",
        "type = \"boolean\"\ndefault = true",
        "type = \"choice\"\nchoices = [\"a\"]\ndefault = \"a\"",
    ] {
        let source = format!("{DECLARATION}{}", setting("extra", rest));
        assert_eq!(parse(&source).unwrap().descriptor_version(), 2, "{rest}");
    }
}

#[test]
fn a_schema_gives_its_settings_by_id() {
    let schema = parse(DECLARATION).unwrap();
    assert_eq!(schema.module(), "test/module");
    assert_eq!(schema.name(), "Test");
    assert_eq!(schema.settings().len(), 2);
    assert_eq!(schema.setting("active").unwrap().label(), "Animation");
    assert!(schema.setting("readAtInit").is_none());
    assert_eq!(schema.source(), DECLARATION);
}

#[test]
fn a_module_without_workbench_toml_has_no_schema() {
    let dir = crate::build::toolchain::TempDir::new("pk-schema-").unwrap();
    assert!(
        Schema::read(dir.path(), "test/module", Some("Test"))
            .unwrap()
            .is_none()
    );
    std::fs::write(dir.path().join("workbench.toml"), DECLARATION).unwrap();
    assert!(
        Schema::read(dir.path(), "test/module", None)
            .unwrap_err()
            .0
            .contains("module.toml")
    );
    assert!(
        Schema::read(dir.path(), "test/module", Some("Test"))
            .unwrap()
            .is_some()
    );
    std::fs::write(
        dir.path().join("workbench.toml"),
        "[workbench]\nversion = 3",
    )
    .unwrap();
    let error = Schema::read(dir.path(), "test/module", Some("Test")).unwrap_err();
    assert!(
        error.0.contains("workbench.toml") && error.0.contains("version"),
        "{error}"
    );
}
