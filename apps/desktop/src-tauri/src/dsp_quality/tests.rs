use super::*;

#[test]
fn catalog_finds_named_rig_modules_without_building_their_objects() {
    let checkout = tempfile::tempdir().unwrap();
    let root = checkout.path();
    for (id, name, kind, case) in [
        ("acme/a", "Zither", "machine", "case.toml"),
        ("acme/z", "Delay", "fx", "case.toml"),
        ("acme/no-rig", "Source Only", "machine", "rig.rig.test.ts"),
        ("acme/tool", "Tool", "raw", "case.toml"),
        ("test/fixture", "Fixture", "machine", "case.toml"),
    ] {
        let (category, _) = id.split_once('/').unwrap();
        let module = root.join("modules").join(id);
        let rig = module.join("tests/rig");
        fs::create_dir_all(&rig).unwrap();
        fs::write(rig.join(case), "").unwrap();
        fs::write(
            module.join("module.toml"),
            format!(
                "[module]\nid = \"{id}\"\nname = \"{name}\"\ntype = \"{kind}\"\ncategory = \"{category}\"\n\
                 version = \"0.1.0\"\nauthor = \"{category}\"\nstatus = \"experimental\"\nsummary = \"A test module\"\n\
                 [[object]]\npath = \"absent.dko\"\nplace = \"example\"\nsources = [\"absent.c\"]\n"
            ),
        )
        .unwrap();
    }
    assert_eq!(
        modules(root).unwrap(),
        [
            DspQualityModule {
                id: "acme/z".into(),
                name: "Delay".into()
            },
            DspQualityModule {
                id: "acme/a".into(),
                name: "Zither".into()
            },
        ]
    );
}

#[test]
fn module_id_is_two_safe_components() {
    assert!(valid_module_id("acme/short-delay"));
    assert!(valid_module_id("acme/lfo_shapes_1"));
    for id in [
        "effect",
        "/effect",
        "acme/",
        "a/b/c",
        "../effect",
        "a/..",
        "--out/a",
        "a/b c",
        "a/é",
        "A/b",
        r"a\b/c",
    ] {
        assert!(!valid_module_id(id), "{id:?}");
    }
}

#[test]
fn reservations_keep_existing_reports() {
    let checkout = tempfile::tempdir().unwrap();
    let root = checkout.path().canonicalize().unwrap();
    let first = reserve_folder(&root).unwrap();
    let report = root.join(&first).join("quality.json");
    fs::write(&report, "first run").unwrap();

    let second = reserve_folder(&root).unwrap();
    assert_ne!(first, second);
    assert!(second.starts_with("out/runs"));
    assert!(root.join(second).is_dir());
    assert_eq!(fs::read_to_string(report).unwrap(), "first run");
}

#[cfg(unix)]
#[test]
fn report_reservation_refuses_an_output_tree_outside_the_checkout() {
    let checkout = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), checkout.path().join("out")).unwrap();

    assert!(reserve_folder(&checkout.path().canonicalize().unwrap()).is_err());
    assert!(!outside.path().join("runs").exists());
}

#[test]
fn a_failed_outcome_is_a_report_and_a_wrong_module_is_an_error() {
    let folder = tempfile::tempdir().unwrap();
    let report = folder.path().join("quality.json");
    let module = "acme/short-delay";
    assert!(check_report(folder.path(), module).is_err());
    fs::write(&report, "{").unwrap();
    assert!(check_report(folder.path(), module).is_err());
    fs::write(
        &report,
        format!(r#"{{"schema":"dk.module-quality/1","module":"{module}","outcome":"fail"}}"#),
    )
    .unwrap();
    assert!(check_report(folder.path(), module).is_ok());
    assert!(check_report(folder.path(), "acme/long-delay").is_err());
}
