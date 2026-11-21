//! `patchkit service-check`: the layout, the one exported entry and the declarations against the manifest of a service crate.
//! The good case is the real template (`sdk/templates/service`), the bad ones are that template broken one way each.

use patchkit::svccheck::check;
use std::fs;
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// A fresh folder for one test.
fn scratch(test: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pk-svc-check-{}-{test}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap().flatten() {
        let p = e.path();
        let target = to.join(e.file_name());
        if p.is_dir() {
            copy_tree(&p, &target);
        } else {
            let text = fs::read_to_string(&p)
                .unwrap()
                .replace("__NAME__", "sweep")
                .replace("__CAMEL__", "Sweep")
                .replace("__UPPER__", "SWEEP")
                .replace("__ID__", "core/services/sweep")
                .replace("__KEBAB__", "sweep");
            fs::write(target, text).unwrap();
        }
    }
}

/// The template as `<scratch>/sweep`, and an `sdk.toml` with the service's row and `kinds` (key and length) as its record kinds.
fn service(test: &str, kinds: &[&str]) -> (PathBuf, PathBuf) {
    let root = scratch(test);
    let dir = root.join("sweep");
    copy_tree(&repo().join("sdk/templates/service"), &dir);
    let mut sdk = String::from(
        "[[service]]\nname = \"sweep\"\nrust = \"dkrt_svc_sweep::records::Sweep\"\n\n[records.kinds]\n",
    );
    for k in kinds {
        sdk.push_str(&format!(
            "{k} = {{ id = 9, len = 16, service = \"sweep\" }}\n"
        ));
    }
    sdk.push_str("OTHER = { id = 2, len = 16, service = \"elsewhere\" }\n");
    let sdk_path = root.join("sdk.toml");
    fs::write(&sdk_path, sdk).unwrap();
    (dir, sdk_path)
}

fn problems(dir: &Path, sdk: &Path) -> Vec<String> {
    check(dir, sdk).unwrap()
}

fn append(path: &Path, text: &str) {
    let mut s = fs::read_to_string(path).unwrap();
    s.push_str(text);
    fs::write(path, s).unwrap();
}

fn mentions(found: &[String], what: &str) -> bool {
    found.iter().any(|p| p.contains(what))
}

#[test]
fn the_template_passes() {
    let (dir, sdk) = service("good", &[]);
    assert_eq!(problems(&dir, &sdk), Vec::<String>::new());
}

#[test]
fn a_missing_file_or_folder_of_the_layout_fails() {
    for (gone, what) in [
        ("src/slots.rs", "src/slots.rs is missing"),
        ("src/records.rs", "src/records.rs is missing"),
        ("Cargo.toml", "Cargo.toml is missing"),
    ] {
        let (dir, sdk) = service("layout-file", &[]);
        fs::remove_file(dir.join(gone)).unwrap();
        assert!(
            mentions(&problems(&dir, &sdk), what),
            "{gone}: {:?}",
            problems(&dir, &sdk)
        );
    }
    let (dir, sdk) = service("layout-tests", &[]);
    fs::remove_dir_all(dir.join("tests")).unwrap();
    assert!(mentions(&problems(&dir, &sdk), "tests/ has no .rs file"));
}

#[test]
fn the_layout_ties_the_crate_name_the_modules_and_the_dsp_folder() {
    let (dir, sdk) = service("layout-name", &[]);
    let cargo = fs::read_to_string(dir.join("Cargo.toml"))
        .unwrap()
        .replace("dkrt-svc-sweep", "dkrt-svc-other");
    fs::write(dir.join("Cargo.toml"), cargo).unwrap();
    assert!(mentions(
        &problems(&dir, &sdk),
        "the layout wants `dkrt-svc-sweep`"
    ));

    let (dir, sdk) = service("layout-mod", &[]);
    fs::write(dir.join("src/lib.rs"), "#![no_std]\npub mod records;\n").unwrap();
    assert!(mentions(
        &problems(&dir, &sdk),
        "does not declare `pub mod slots;`"
    ));

    // a service with DSP handlers has the dsp/ folder, and the folder has a mod.rs
    let (dir, sdk) = service("layout-dsp", &[]);
    append(
        &dir.join("src/records.rs"),
        "impl Sweep {\n    unsafe fn dsp_admit() {}\n}\n",
    );
    assert!(mentions(&problems(&dir, &sdk), "has no src/dsp/ folder"));
    fs::create_dir(dir.join("src/dsp")).unwrap();
    let found = problems(&dir, &sdk);
    assert!(mentions(&found, "src/dsp/ has no mod.rs"), "{found:?}");
    assert!(
        mentions(&found, "does not declare `pub mod dsp;`"),
        "{found:?}"
    );
    fs::write(dir.join("src/dsp/mod.rs"), "").unwrap();
    append(&dir.join("src/lib.rs"), "pub mod dsp;\n");
    assert_eq!(problems(&dir, &sdk), Vec::<String>::new());
}

#[test]
fn the_one_exported_entry_is_dk_module_and_a_second_export_fails() {
    let (dir, sdk) = service("export-ok", &[]);
    append(
        &dir.join("src/records.rs"),
        "#[no_mangle]\npub unsafe extern \"C\" fn dk_module() {}\n",
    );
    assert_eq!(problems(&dir, &sdk), Vec::<String>::new());

    // a second row symbol next to the entry
    append(
        &dir.join("src/slots.rs"),
        "#[no_mangle]\npub static dk_service_row: u32 = 0;\n",
    );
    let found = problems(&dir, &sdk);
    assert!(mentions(&found, "exports `dk_service_row`"), "{found:?}");

    // the entry twice
    let (dir, sdk) = service("export-twice", &[]);
    append(
        &dir.join("src/records.rs"),
        "#[no_mangle]\npub unsafe extern \"C\" fn dk_module() {}\n",
    );
    append(
        &dir.join("src/slots.rs"),
        "#[unsafe(no_mangle)]\npub unsafe extern \"C\" fn dk_module() {}\n",
    );
    assert!(mentions(
        &problems(&dir, &sdk),
        "`dk_module` is defined 2 times"
    ));

    // the state a device test reads by name is not an entry
    let (dir, sdk) = service("export-state", &[]);
    append(
        &dir.join("src/slots.rs"),
        "#[no_mangle]\npub static mut DKRT_SWEEP: u32 = 0;\n",
    );
    assert_eq!(problems(&dir, &sdk), Vec::<String>::new());

    // an export under another name by attribute, and an export of a test build only
    let (dir, sdk) = service("export-name", &[]);
    append(
        &dir.join("src/slots.rs"),
        "#[export_name = \"svc_row\"]\npub static ROW: u32 = 0;\n",
    );
    assert!(mentions(&problems(&dir, &sdk), "exports `svc_row`"));
    let (dir, sdk) = service("export-test", &[]);
    append(
        &dir.join("src/slots.rs"),
        "#[cfg(test)]\nmod stubs {\n    #[no_mangle]\n    pub extern \"C\" fn a_test_stub() {}\n}\n",
    );
    assert_eq!(problems(&dir, &sdk), Vec::<String>::new());
}

#[test]
fn a_declaration_that_differs_from_the_manifest_fails() {
    // declared but not in the manifest
    let (dir, sdk) = service("decl-extra", &[]);
    let rec = fs::read_to_string(dir.join("src/records.rs")).unwrap().replace("unsafe fn declare(_d: &mut Declare) {}", "unsafe fn declare(d: &mut Declare) {\n        d.kind(KIND_SWEEP_STEP, LEN_SWEEP_STEP);\n    }");
    fs::write(dir.join("src/records.rs"), rec).unwrap();
    assert!(mentions(
        &problems(&dir, &sdk),
        "declares the record kind SWEEP_STEP, which the manifest does not give"
    ));

    // in the manifest but not declared
    let (dir, sdk) = service("decl-missing", &["SWEEP_STEP"]);
    assert!(mentions(
        &problems(&dir, &sdk),
        "gives the record kind SWEEP_STEP to `sweep`, but records.rs does not declare it"
    ));

    // declared and in the manifest: equal; another service's kind is not this one's
    let (dir, sdk) = service("decl-equal", &["SWEEP_STEP"]);
    let rec = fs::read_to_string(dir.join("src/records.rs")).unwrap().replace("unsafe fn declare(_d: &mut Declare) {}", "unsafe fn declare(d: &mut Declare) {\n        d.kind(dkrt_sdk::records::KIND_SWEEP_STEP, dkrt_sdk::records::LEN_SWEEP_STEP);\n    }");
    fs::write(dir.join("src/records.rs"), rec).unwrap();
    assert_eq!(problems(&dir, &sdk), Vec::<String>::new());

    // a length of another kind, and a declaration that has no manifest entry at all
    let (dir, sdk) = service("decl-pair", &["SWEEP_STEP"]);
    let rec = fs::read_to_string(dir.join("src/records.rs")).unwrap().replace("unsafe fn declare(_d: &mut Declare) {}", "unsafe fn declare(d: &mut Declare) {\n        d.kind(KIND_SWEEP_STEP, LEN_OTHER);\n        d.slot(1, 0, 0);\n    }");
    fs::write(dir.join("src/records.rs"), rec).unwrap();
    let found = problems(&dir, &sdk);
    assert!(
        mentions(&found, "declares `kind(KIND_SWEEP_STEP, LEN_OTHER)`"),
        "{found:?}"
    );
    assert!(mentions(&found, "declares `slot`"), "{found:?}");
}

#[test]
fn a_service_needs_local_exports_or_an_sdk_row_with_a_type_of_its_crate() {
    let (dir, sdk) = service("row-missing", &[]);
    fs::write(&sdk, "[records.kinds]\n").unwrap();
    assert!(mentions(
        &problems(&dir, &sdk),
        "needs at least one [[export]]"
    ));
    fs::write(
        &sdk,
        "[[service]]\nname = \"sweep\"\nrust = \"crate::services::sweep::Sweep\"\n",
    )
    .unwrap();
    assert!(mentions(
        &problems(&dir, &sdk),
        "the type is not in the crate `dkrt_svc_sweep`"
    ));
}

#[test]
fn the_service_file_layout_id_and_role_are_checked() {
    for (gone, what) in [
        ("dks/module.toml", "dks/module.toml is missing"),
        ("dks/Cargo.toml", "dks/Cargo.toml is missing"),
        ("dks/src/lib.rs", "dks/src/lib.rs is missing"),
    ] {
        let (dir, sdk) = service("dks-file", &[]);
        fs::remove_file(dir.join(gone)).unwrap();
        assert!(
            mentions(&problems(&dir, &sdk), what),
            "{gone}: {:?}",
            problems(&dir, &sdk)
        );
    }
    let (dir, sdk) = service("dks-id", &[]);
    let m = fs::read_to_string(dir.join("dks/module.toml")).unwrap();
    fs::write(
        dir.join("dks/module.toml"),
        m.replace("core/services/sweep", "core/sweep"),
    )
    .unwrap();
    assert!(mentions(
        &problems(&dir, &sdk),
        "the layout wants `core/services/sweep`"
    ));
    let (dir, sdk) = service("dks-role", &[]);
    let m = fs::read_to_string(dir.join("dks/module.toml")).unwrap();
    fs::write(
        dir.join("dks/module.toml"),
        m.replace("role = \"service\"", "role = \"tool\""),
    )
    .unwrap();
    assert!(mentions(&problems(&dir, &sdk), "`role` is not \"service\""));
    // a service whose row has no `rust` is a file: dks/ is required
    let (dir, sdk) = service("dks-required", &[]);
    fs::remove_dir_all(dir.join("dks")).unwrap();
    assert_eq!(
        problems(&dir, &sdk),
        Vec::<String>::new(),
        "a linked-in service has no dks/"
    );
    fs::write(&sdk, "[[service]]\nname = \"sweep\"\n").unwrap();
    assert!(mentions(&problems(&dir, &sdk), "dks/ is missing"));
}

#[test]
fn dks_lib_rs_is_the_service_call_alone() {
    let (dir, sdk) = service("dks-lib", &[]);
    append(&dir.join("dks/src/lib.rs"), "\nfn helper() {}\n");
    assert!(mentions(
        &problems(&dir, &sdk),
        "dks/src/lib.rs is not the `service!(...)` call alone"
    ));
    // the extra items go to hooks.rs or fw.rs, which lib.rs declares
    let (dir, sdk) = service("dks-lib-mod", &[]);
    append(&dir.join("dks/src/lib.rs"), "\nmod fw;\n");
    assert_eq!(problems(&dir, &sdk), Vec::<String>::new());
}

#[test]
fn service_hooks_equal_the_hook_entries_per_device() {
    let call = |hooks: &str| {
        format!(
            "#![no_std]\nmod hooks;\ndkrt_svc_rt::service!(Sweep, \"sweep\", &KINDS_SWEEP, setup = hooks::setup, hooks = [{hooks}]);\n"
        )
    };
    let entry =
        |name: &str, dev: &str| format!("\n[[hook]]\nname = \"{name}\"\nentry = \"e\"\n{dev}");
    // equal: one hook for both devices, one for dn2 only
    let (dir, sdk) = service("hooks-equal", &[]);
    fs::write(
        dir.join("dks/src/lib.rs"),
        call("(\"cf.a.hook.x\", hooks::x), (\"cf.a.hook.y\", hooks::y, dn2)"),
    )
    .unwrap();
    fs::write(dir.join("dks/src/hooks.rs"), "").unwrap();
    append(&dir.join("dks/module.toml"), &entry("cf.a.hook.x", ""));
    append(
        &dir.join("dks/module.toml"),
        &entry("cf.a.hook.y", "devices = [\"dn2\"]\n"),
    );
    assert_eq!(problems(&dir, &sdk), Vec::<String>::new());
    // the call has a hook the manifest has not, and the manifest has one the call has not, each for its device
    fs::write(
        dir.join("dks/src/lib.rs"),
        call("(\"cf.a.hook.x\", hooks::x), (\"cf.a.hook.y\", hooks::y, dt2)"),
    )
    .unwrap();
    let found = problems(&dir, &sdk);
    assert!(
        mentions(&found, "service! takes the hook `cf.a.hook.y` on dt2"),
        "{found:?}"
    );
    assert!(
        mentions(
            &found,
            "[[hook]] `cf.a.hook.y` on dn2 is not in the service! hook list"
        ),
        "{found:?}"
    );
    // [[hook]] entries need hooks.rs
    fs::remove_file(dir.join("dks/src/hooks.rs")).unwrap();
    assert!(mentions(
        &problems(&dir, &sdk),
        "dks/src/hooks.rs is missing"
    ));
}

/// The service with an adapters folder beside it that declares `[[slot]] frametype` with the given `devices` line.
fn adapters_with(test: &str, devices: &str) -> (PathBuf, PathBuf, PathBuf) {
    let (dir, sdk) = service(test, &[]);
    let adapters = dir.parent().unwrap().join("adapters/machines");
    fs::create_dir_all(&adapters).unwrap();
    fs::write(
        adapters.join("module.toml"),
        format!("[[slot]]\nname = \"frametype\"\n{devices}\n"),
    )
    .unwrap();
    (dir, sdk, adapters.parent().unwrap().to_path_buf())
}

/// The `service!` call of the service in `dir` with `extra` after `hooks = []` (the fills and exports of the call must equal the manifest entries).
fn call_with(dir: &Path, extra: &str) {
    fs::write(dir.join("dks/src/hooks.rs"), "").unwrap();
    fs::write(
        dir.join("dks/src/lib.rs"),
        format!(
            "#![no_std]\nmod hooks;\ndkrt_svc_rt::service!(Sweep, \"sweep\", &KINDS_SWEEP, setup = hooks::setup, hooks = [], {extra});\n"
        ),
    )
    .unwrap();
}

#[test]
fn service_fills_equal_the_manifest_entries() {
    let call = |extra: &str| {
        format!(
            "#![no_std]\nmod hooks;\ndkrt_svc_rt::service!(Sweep, \"sweep\", &KINDS_SWEEP, setup = hooks::setup, hooks = [], {extra});\n"
        )
    };
    let (dir, sdk, adapters) = adapters_with("fills", "devices = [\"dn2\"]");
    let problems = |dir: &Path, sdk: &Path| {
        patchkit::svccheck::check_with_adapters(dir, sdk, &adapters).unwrap()
    };
    fs::write(dir.join("dks/src/hooks.rs"), "").unwrap();
    fs::write(
        dir.join("dks/src/lib.rs"),
        call("fills = [(\"frametype\", hooks::f, dn2)]"),
    )
    .unwrap();
    // the call has a fill the manifest has not
    let found = problems(&dir, &sdk);
    assert!(
        mentions(&found, "service! takes the fill `frametype` on dn2"),
        "{found:?}"
    );
    // the manifest has it (for dn2 only)
    append(
        &dir.join("dks/module.toml"),
        "\n[[fill]]\nslot = \"frametype\"\nentry = \"f\"\ndevices = [\"dn2\"]\n",
    );
    assert_eq!(problems(&dir, &sdk), Vec::<String>::new());
}

#[test]
fn service_exports_are_the_generated_list_of_the_provided_imports() {
    let call = |exports: &str| {
        format!(
            "#![no_std]\nmod hooks;\nmod gen_exports;\ndkrt_svc_rt::service!(Sweep, \"sweep\", &KINDS_SWEEP, setup = hooks::setup, hooks = [], fills = []{exports});\n"
        )
    };
    let provided = "\n[[import]]\nname = \"dk_sweep_go\"\nservice = \"sweep\"\nprovided = true\n\n[[import]]\nname = \"dk_other\"\nservice = \"elsewhere\"\nprovided = true\n";
    // no provided import: no exports, and the generated list is refused
    let (dir, sdk) = service("exports-none", &[]);
    fs::write(dir.join("dks/src/hooks.rs"), "").unwrap();
    fs::write(dir.join("dks/src/gen_exports.rs"), "").unwrap();
    fs::write(dir.join("dks/src/lib.rs"), call("")).unwrap();
    assert_eq!(problems(&dir, &sdk), Vec::<String>::new());
    fs::write(
        dir.join("dks/src/lib.rs"),
        call(", exports = gen_exports::exports()"),
    )
    .unwrap();
    assert!(mentions(
        &problems(&dir, &sdk),
        "service has no `provided` import"
    ));
    // a provided import of the service: the call must take the generated list (an import of another service does not count)
    append(&sdk, provided);
    // a stale generated list (not the provided imports, in order) is refused
    fs::write(
        dir.join("dks/src/lib.rs"),
        call(", exports = gen_exports::exports()"),
    )
    .unwrap();
    assert!(mentions(
        &problems(&dir, &sdk),
        "does not list the `provided` imports"
    ));
    let listed =
        "pub fn exports() { [(dkrt_svc_rt::name_hash(\"dk_sweep_go\"), f as *const u8)] }\n";
    fs::write(dir.join("dks/src/gen_exports.rs"), listed).unwrap();
    let found = problems(&dir, &sdk);
    assert_eq!(found, Vec::<String>::new(), "{found:?}");
    // a test-only provided import is an export of the file too
    append(
        &sdk,
        "\n[[import]]\nname = \"dk_sweep_t\"\nservice = \"sweep\"\nprovided = true\ntest = true\n",
    );
    assert!(mentions(
        &problems(&dir, &sdk),
        "does not list the `provided` imports"
    ));
    fs::write(
        dir.join("dks/src/gen_exports.rs"),
        format!("{listed}(dkrt_svc_rt::name_hash(\"dk_sweep_t\"), t as *const u8)\n"),
    )
    .unwrap();
    let found = problems(&dir, &sdk);
    assert_eq!(found, Vec::<String>::new(), "{found:?}");
    fs::write(dir.join("dks/src/lib.rs"), call("")).unwrap();
    assert!(mentions(
        &problems(&dir, &sdk),
        "service! needs `exports = gen_exports::exports()`"
    ));
    // a literal list is refused, with or without provided imports
    fs::write(
        dir.join("dks/src/lib.rs"),
        call(", exports = [(\"dk_sweep_go\", fw::dk_sweep_go)]"),
    )
    .unwrap();
    assert!(mentions(
        &problems(&dir, &sdk),
        "literal `exports = [...]` list"
    ));
    // the generated file must exist
    fs::write(
        dir.join("dks/src/lib.rs"),
        call(", exports = gen_exports::exports()"),
    )
    .unwrap();
    fs::remove_file(dir.join("dks/src/gen_exports.rs")).unwrap();
    assert!(mentions(
        &problems(&dir, &sdk),
        "dks/src/gen_exports.rs is missing"
    ));
    // an SDK service exports only its provided imports (and the optional resident query)
    fs::write(dir.join("dks/src/gen_exports.rs"), listed).unwrap();
    append(
        &dir.join("dks/module.toml"),
        "\n[[export]]\nname = \"dk_sweep_go\"\n",
    );
    assert!(mentions(
        &problems(&dir, &sdk),
        "SDK service exports are the `provided = true` imports"
    ));
}

#[test]
fn local_exports_match_by_name_and_ignore_commented_out_tuples() {
    let (dir, sdk) = service("local-export", &[]);
    fs::write(&sdk, "[records.kinds]\n").unwrap();
    append(
        &dir.join("dks/module.toml"),
        "\n[[export]]\nname = \"sweep_first\"\n\n[[export]]\nname = \"sweep_second\"\n",
    );
    let source = r#"
mod api;
dkrt_svc_rt::service!(
    Sweep, "sweep", &[],
    // This opening block-comment marker is part of a line comment: /*
    setup = api::setup,
    hooks = [],
    fills = [],
    exports = [
        ("sweep_second", api::second),
        /* Keep this example out: /* nested comment */ ("unused", api::unused), */
        ("sweep_first", api::first),
    ]
);
"#;
    let entry = dir.join("dks/src/lib.rs");
    fs::write(&entry, source).unwrap();
    assert_eq!(problems(&dir, &sdk), Vec::<String>::new());
    fs::write(&entry, source.replace("sweep_second", "sweep_first")).unwrap();
    assert!(mentions(
        &problems(&dir, &sdk),
        "exports must match the names"
    ));
}

#[test]
fn assembler_and_firmware_names_stay_in_their_place() {
    // assembler: only dks/src/hooks.rs; the neutral crate has none
    let (dir, sdk) = service("asm", &[]);
    fs::write(
        dir.join("dks/src/hooks.rs"),
        "core::arch::global_asm!(\".text\");\n",
    )
    .unwrap();
    append(&dir.join("dks/src/lib.rs"), "\nmod hooks;\n");
    assert_eq!(problems(&dir, &sdk), Vec::<String>::new());
    fs::write(dir.join("dks/src/fw.rs"), "core::arch::asm!(\"nop\");\n").unwrap();
    assert!(mentions(
        &problems(&dir, &sdk),
        "dks/src/fw.rs: assembler is only in dks/src/hooks.rs"
    ));
    let (dir, sdk) = service("asm-neutral", &[]);
    append(
        &dir.join("src/slots.rs"),
        "pub fn f() { unsafe { core::arch::asm!(\"nop\") } }\n",
    );
    assert!(mentions(
        &problems(&dir, &sdk),
        "src/slots.rs: assembler is only in dks/src/hooks.rs"
    ));
    // firmware names: allowed under dks/, refused in the neutral crate
    let (dir, sdk) = service("fw-names", &[]);
    append(&dir.join("dks/src/lib.rs"), "\nmod fw;\n");
    fs::write(
        dir.join("dks/src/fw.rs"),
        "use dkfw::page;\nconst H: &str = \"cf.page.hook.x\";\n",
    )
    .unwrap();
    assert_eq!(problems(&dir, &sdk), Vec::<String>::new());
    append(
        &dir.join("src/slots.rs"),
        "pub const A: &str = \"cf.page.fn.draw\";\n",
    );
    append(&dir.join("src/records.rs"), "use dkfw::page;\n");
    let found = problems(&dir, &sdk);
    assert!(
        mentions(&found, "names firmware (`\"cf.page.fn.draw\"`)"),
        "{found:?}"
    );
    assert!(mentions(&found, "names firmware (`dkfw`)"), "{found:?}");
}

/// The four service crates of the runtime follow the layout today.
#[test]
fn the_service_crates_of_the_repository_pass() {
    let sdk = repo().join("sdk/sdk.toml");
    for name in ["fx", "machines", "samples", "lfo"] {
        let dir = repo().join("services").join(name);
        assert_eq!(problems(&dir, &sdk), Vec::<String>::new(), "{name}");
    }
}

/// A DSP fill: the manifest side (`[[fill]] space = "dsp"`, an `[[object]]` with place `dsp.l2`, `dks/dsp/*.s`) and a dyndsp table next to the service folder.
fn with_dsp_fill(test: &str, asm_name: &str, asm: &str) -> (PathBuf, PathBuf, PathBuf) {
    let (dir, sdk, adapters) = adapters_with(test, "devices = [\"dn2\"]");
    // the adapter that declares the DSP slot
    fs::write(
        adapters.join("machines/module.toml"),
        "[[slot]]\nname = \"voice_render\"\nspace = \"dsp\"\ndevices = [\"dt2\"]\n",
    )
    .unwrap();
    let table = dir.parent().unwrap().join("runtime/platform/dyndsp/src/cf");
    fs::create_dir_all(&table).unwrap();
    fs::write(
        table.join("dyndsp-dt2.s"),
        "dyndsp_info:\n    DSP_IMPORT _dv_diag\n    DSP_IMPORT _fw_frame\n",
    )
    .unwrap();
    fs::create_dir_all(dir.join("dks/dsp")).unwrap();
    fs::write(dir.join("dks/dsp").join(asm_name), asm).unwrap();
    append(
        &dir.join("dks/module.toml"),
        "\n[[fill]]\nslot = \"voice_render\"\nentry = \"_run\"\nspace = \"dsp\"\ndevices = [\"dt2\"]\n\n[[object]]\npath = \"build/x.dko.json\"\nsources = [\"dsp/dispatch-dt2.s\"]\nplace = \"dsp.l2\"\ndevices = [\"dt2\"]\n",
    );
    (dir, sdk, adapters)
}

#[test]
fn a_dsp_fill_needs_dispatch_code_and_imports_that_the_dyndsp_table_has() {
    // a DSP fill is not a Rust function: the service! call does not list it, and the check wants the dispatch code and a DSP object
    let found_in =
        |d: &Path, s: &Path, a: &Path| patchkit::svccheck::check_with_adapters(d, s, a).unwrap();
    let asm = ".EXTERN _dv_diag;\n.EXTERN _fw_frame;\n";
    let (dir, sdk, ad) = with_dsp_fill("dsp-ok", "dispatch-dt2.s", asm);
    assert_eq!(found_in(&dir, &sdk, &ad), Vec::<String>::new());

    let (dir, sdk, ad) = with_dsp_fill(
        "dsp-bad-import",
        "dispatch-dt2.s",
        ".EXTERN _dv_diag;\n.EXTERN _dv_nowhere;\n",
    );
    let found = found_in(&dir, &sdk, &ad);
    assert!(
        mentions(
            &found,
            "`.EXTERN _dv_nowhere` is not a DSP_IMPORT of core/dyndsp"
        ),
        "{found:?}"
    );
    assert!(!mentions(&found, "_dv_diag"), "{found:?}");

    let (dir, sdk, ad) = with_dsp_fill("dsp-no-asm", "notes.txt", "");
    assert!(mentions(&found_in(&dir, &sdk, &ad), "holds no .s file"));

    let (dir, sdk, ad) = with_dsp_fill("dsp-no-device", "dispatch.s", asm);
    assert!(mentions(
        &found_in(&dir, &sdk, &ad),
        "must end in -<device>"
    ));

    // an unknown slot name
    let (dir, sdk, ad) = with_dsp_fill("dsp-bad-slot", "dispatch-dt2.s", asm);
    let m = fs::read_to_string(dir.join("dks/module.toml")).unwrap();
    fs::write(
        dir.join("dks/module.toml"),
        m.replace("voice_render", "no_such_slot"),
    )
    .unwrap();
    assert!(mentions(&found_in(&dir, &sdk, &ad), "has no such slot"));
}

#[test]
fn a_fill_names_the_devices_of_the_adapter_slot_it_fills() {
    use patchkit::svccheck::check_with_adapters;
    let fill =
        |devices: &str| format!("\n[[fill]]\nslot = \"frametype\"\nentry = \"e\"\n{devices}");
    // a slot on dn2 only: a fill for dn2 agrees
    let (dir, sdk, adapters) = adapters_with("fill-ok", "devices = [\"dn2\"]");
    append(&dir.join("dks/module.toml"), &fill("devices = [\"dn2\"]\n"));
    call_with(&dir, "fills = [(\"frametype\", hooks::e, dn2)]");
    assert_eq!(
        check_with_adapters(&dir, &sdk, &adapters).unwrap(),
        Vec::<String>::new()
    );
    // a fill with no devices would fill a slot that dt2 does not have
    let (dir, sdk, adapters) = adapters_with("fill-all", "devices = [\"dn2\"]");
    append(&dir.join("dks/module.toml"), &fill(""));
    let found = check_with_adapters(&dir, &sdk, &adapters).unwrap();
    assert!(
        mentions(&found, "[[fill]] `frametype` has no `devices`"),
        "{found:?}"
    );
    // a device the slot lacks
    let (dir, sdk, adapters) = adapters_with("fill-extra", "devices = [\"dn2\"]");
    append(
        &dir.join("dks/module.toml"),
        &fill("devices = [\"dn2\", \"dt2\"]\n"),
    );
    let found = check_with_adapters(&dir, &sdk, &adapters).unwrap();
    assert!(
        mentions(
            &found,
            "is for dt2, but the image adapter has the slot only on dn2"
        ),
        "{found:?}"
    );
    // a slot on every device takes a fill without devices
    let (dir, sdk, adapters) = adapters_with("fill-slot-all", "");
    append(&dir.join("dks/module.toml"), &fill(""));
    call_with(&dir, "fills = [(\"frametype\", hooks::e)]");
    assert_eq!(
        check_with_adapters(&dir, &sdk, &adapters).unwrap(),
        Vec::<String>::new()
    );
    // a fill that names no slot of any adapter is a typo
    let (dir, sdk, adapters) = adapters_with("fill-typo", "devices = [\"dn2\"]");
    append(
        &dir.join("dks/module.toml"),
        "\n[[fill]]\nslot = \"frametpye\"\nentry = \"e\"\ndevices = [\"dn2\"]\n",
    );
    let found = check_with_adapters(&dir, &sdk, &adapters).unwrap();
    assert!(
        mentions(
            &found,
            "`frametpye` names a slot that no image adapter declares"
        ),
        "{found:?}"
    );
}

/// The service with two adapter folders, each declaring `[[slot]] frametype` with the given `devices` lines.
fn two_adapters(test: &str, first: &str, second: &str) -> (PathBuf, PathBuf, PathBuf) {
    let (dir, sdk) = service(test, &[]);
    let adapters = dir.parent().unwrap().join("adapters");
    for (folder, devices) in [("machines", first), ("other", second)] {
        fs::create_dir_all(adapters.join(folder)).unwrap();
        fs::write(
            adapters.join(folder).join("module.toml"),
            format!("[[slot]]\nname = \"frametype\"\n{devices}\n"),
        )
        .unwrap();
    }
    (dir, sdk, adapters)
}

#[test]
fn a_slot_declared_twice_for_a_common_device_fails_and_on_disjoint_devices_passes() {
    use patchkit::svccheck::check_with_adapters;
    let twice = "the slot `frametype` is declared twice";
    let (dir, sdk, adapters) = two_adapters(
        "slot-twice",
        "devices = [\"dn2\"]",
        "devices = [\"dn2\", \"dt2\"]",
    );
    let found = check_with_adapters(&dir, &sdk, &adapters).unwrap();
    assert!(mentions(&found, twice), "{found:?}");
    // a slot with no `devices` is on every device, so any second declaration overlaps
    let (dir, sdk, adapters) = two_adapters("slot-twice-all", "", "devices = [\"dt2\"]");
    let found = check_with_adapters(&dir, &sdk, &adapters).unwrap();
    assert!(mentions(&found, twice), "{found:?}");
    // one slot per device is not a duplicate, and a fill for either device agrees
    let (dir, sdk, adapters) = two_adapters(
        "slot-disjoint",
        "devices = [\"dn2\"]",
        "devices = [\"dt2\"]",
    );
    append(
        &dir.join("dks/module.toml"),
        "\n[[fill]]\nslot = \"frametype\"\nentry = \"e\"\ndevices = [\"dn2\", \"dt2\"]\n",
    );
    call_with(&dir, "fills = [(\"frametype\", hooks::e)]");
    assert_eq!(
        check_with_adapters(&dir, &sdk, &adapters).unwrap(),
        Vec::<String>::new()
    );
}

#[test]
fn a_fill_without_the_adapters_folder_is_an_error() {
    use patchkit::svccheck::check_with_adapters;
    let (dir, sdk) = service("no-adapters", &[]);
    append(
        &dir.join("dks/module.toml"),
        "\n[[fill]]\nslot = \"frametype\"\nentry = \"e\"\n",
    );
    let missing = dir.parent().unwrap().join("no-such-adapters");
    let found = check_with_adapters(&dir, &sdk, &missing).unwrap();
    assert!(mentions(&found, "the adapters folder"), "{found:?}");
    assert!(mentions(&found, "is missing"), "{found:?}");
}

#[test]
fn a_row_anchor_has_one_service_in_its_users() {
    let fw = scratch("row-anchor").join("fw");
    fs::create_dir_all(&fw).unwrap();
    let row = |name: &str, users: &str, rule: &str| {
        format!("[[data]]\nname = \"{name}\"\ncpu = \"cf\"\nrule = {rule}\nusers = {users}\n\n")
    };
    let words = "{ kind = \"cf_words\", words = [\"*\", 5] }";
    fs::write(
        fw.join("a.toml"),
        row("cf.a.data.row_ok", "[\"core/services/lfo\"]", words)
            + &row("cf.a.data.row_shared", "[\"core/services/lfo\", \"core/services/fx\"]", words)
            // a row that no service names, a service and a core module, and a code site (not a row)
            + &row("cf.a.data.row_core", "[\"core/params\", \"core/services/lfo\"]", words)
            + &row(
                "cf.a.data.site",
                "[\"core/services/lfo\", \"core/services/fx\"]",
                "{ kind = \"cf_words\", as = \"code\", words = [1], offset = 0, len = 2 }",
            ),
    )
    .unwrap();
    let found = patchkit::svccheck::row_anchor_problems("lfo", &fw);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        mentions(&found, "`cf.a.data.row_shared` has 2 services in `users`"),
        "{found:?}"
    );
    // a missing folder is no problem
    assert!(patchkit::svccheck::row_anchor_problems("lfo", &fw.join("none")).is_empty());
}
