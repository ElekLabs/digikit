//! `patchkit services`: the services that packed fixture modules need, from their NEEDS and the manifests. The packing tests skip when the m68k-elf
//! toolchain is not installed. The sidecar check of a packed file is unit-tested in src/services.rs.

use patchkit::build::toolchain::Env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

const FIXTURE_DEVICE: &str = "dt2";

fn toolchain() -> bool {
    Env::new(&root()).m68k_or_skip()
}

fn patchkit(args: &[&str]) -> String {
    patchkit_with_modules(args, None)
}

fn patchkit_with_modules(args: &[&str], modules: Option<&Path>) -> String {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_patchkit"));
    cmd.arg("--root").arg(root()).args(args);
    if let Some(modules) = modules {
        cmd.env("PATCHKIT_MODULES", modules);
    }
    let r = cmd.output().unwrap();
    assert!(
        r.status.success(),
        "patchkit {args:?} failed:\n{}",
        String::from_utf8_lossy(&r.stderr)
    );
    String::from_utf8(r.stdout).unwrap()
}

/// Pack the fixtures `ids` for dt2 into a scratch folder.
fn pack(ids: &[&str]) -> (PathBuf, Vec<String>) {
    let dir = std::env::temp_dir().join(format!(
        "pk-services-{}-{}",
        std::process::id(),
        ids.join("-").replace('/', "_")
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let files = ids
        .iter()
        .map(|id| {
            let out = dir.join(format!("{}.dkm", id.rsplit('/').next().unwrap()));
            patchkit(&[
                "pack",
                id,
                "--device",
                FIXTURE_DEVICE,
                "-o",
                out.to_str().unwrap(),
            ]);
            out.to_string_lossy().into_owned()
        })
        .collect();
    (dir, files)
}

/// The `service` names of the JSON lines of `patchkit services --device dt2 FILES...`.
fn services(files: &[String]) -> Vec<String> {
    let mut args = vec!["services", "--device", "dt2"];
    args.extend(files.iter().map(String::as_str));
    patchkit(&args)
        .lines()
        .map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            assert_eq!(
                v["store_path"],
                format!("services/{}.dks", v["service"].as_str().unwrap())
            );
            v["service"].as_str().unwrap().to_string()
        })
        .collect()
}

#[test]
fn a_machine_needs_machines_an_effect_needs_fx_and_a_module_without_needs_gets_nothing() {
    if !toolchain() {
        return;
    }
    let (dir, files) = pack(&["test/dyn-dn2", "test/dyn-fx-gain", "test/dyn-demo"]);
    assert_eq!(services(&files[0..1]), ["machines"]);
    assert_eq!(services(&files[1..2]), ["fx"]);
    assert!(services(&files[2..3]).is_empty());
    assert_eq!(
        services(&files),
        ["fx", "machines"],
        "joined once, in name order"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn listing_and_staging_follow_both_kinds_of_transitive_service_need() {
    if !toolchain() {
        return;
    }
    use dkneeds::keys::name_hash;
    use std::fs;
    let dir = std::env::temp_dir().join(format!("pk-services-chain-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let modules = dir.join("modules");
    let out = dir.join("packed");
    let client = dir.join("chain-user.dkm");
    let base_client = root().join("tests/fixtures/modules/local-ping-user");
    let manifest = fs::read_to_string(base_client.join("module.toml")).unwrap();
    let src = fs::read_to_string(base_client.join("src/c/user.c")).unwrap();
    let relay = modules.join("chain-relay");
    let user = modules.join("chain-user");
    fs::create_dir_all(relay.join("src/c")).unwrap();
    fs::create_dir_all(user.join("src/c")).unwrap();
    fs::write(
        relay.join("module.toml"),
        manifest
            .replace("test/local-ping-user", "test/chain-relay")
            .replace("version = \"0.1.0\"", "version = \"1.2.0\"")
            .replace(
                "category = \"test\"",
                "category = \"test\"\nrole = \"service\"\nservice = \"chain-relay\"",
            )
            .replace(
                "[[object]]",
                "[[export]]\nname = \"chain_relay_ping\"\n\n[[object]]",
            ),
    )
    .unwrap();
    fs::write(
        relay.join("src/c/user.c"),
        format!(
            r#"#include <dk/core/module.h>
#include <dk/gen/imports.h>
uint32_t local_ping_value(void);
uint32_t chain_relay_ping(void)
{{
    (void)dk_project_status();
    return local_ping_value();
}}
static int32_t init(dk_module_ctx *ctx)
{{
    return dk_service_declare_export(ctx, {}u, (const void *)chain_relay_ping);
}}
const dk_module_def dk_module = {{.init = init}};
"#,
            name_hash("chain_relay_ping")
        ),
    )
    .unwrap();
    fs::write(
        user.join("module.toml"),
        manifest
            .replace("test/local-ping-user", "test/chain-user")
            .replace("service = \"local-ping\"", "service = \"chain-relay\"")
            .replace("local_ping_value", "chain_relay_ping"),
    )
    .unwrap();
    fs::write(
        user.join("src/c/user.c"),
        src.replace("local_ping_value", "chain_relay_ping"),
    )
    .unwrap();
    patchkit_with_modules(
        &[
            "pack",
            "test/chain-user",
            "--device",
            FIXTURE_DEVICE,
            "-o",
            client.to_str().unwrap(),
        ],
        Some(&modules),
    );

    let listed = patchkit_with_modules(
        &[
            "services",
            "--device",
            FIXTURE_DEVICE,
            client.to_str().unwrap(),
        ],
        Some(&modules),
    );
    let staged = patchkit_with_modules(
        &[
            "services",
            "--device",
            FIXTURE_DEVICE,
            "--pack-dir",
            out.to_str().unwrap(),
            client.to_str().unwrap(),
        ],
        Some(&modules),
    );
    for output in [listed, staged] {
        let lines: Vec<_> = output
            .lines()
            .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
            .collect();
        let names: Vec<_> = lines
            .iter()
            .map(|l| l["service"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["chain-relay", "local-ping", "project"]);
    }
    assert!(out.join("chain-relay.dks").is_file());
    assert!(out.join("local-ping.dks").is_file());
    assert!(out.join("project.dks").is_file());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_named_service_is_wanted_without_a_file_and_another_module_is_refused() {
    let out = patchkit(&[
        "services",
        "--device",
        "dt2",
        "--module",
        "core/services/fx",
    ]);
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(
        (v["service"].as_str(), v["module_id"].as_str()),
        (Some("fx"), Some("core/services/fx"))
    );
    let out = patchkit(&["services", "--device", "dt2", "--service", "machines"]);
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["module_id"], "core/services/machines");
    let r = Command::new(env!("CARGO_BIN_EXE_patchkit"))
        .arg("--root")
        .arg(root())
        .args(["services", "--device", "dt2", "--module", "test/dyn-dn2"])
        .output()
        .unwrap();
    assert!(!r.status.success());
    assert!(String::from_utf8_lossy(&r.stderr).contains("not a service module"));
}
