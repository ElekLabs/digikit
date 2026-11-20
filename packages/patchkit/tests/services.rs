//! `patchkit services`: the services that packed fixture modules need, from their NEEDS and the manifests. The packing tests skip when the m68k-elf
//! toolchain is not installed. The sidecar check of a packed file is unit-tested in src/services.rs.

use patchkit::build::toolchain::Env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn toolchain() -> bool {
    Env::new(&root()).m68k_or_skip()
}

fn patchkit(args: &[&str]) -> String {
    let r = Command::new(env!("CARGO_BIN_EXE_patchkit"))
        .arg("--root")
        .arg(root())
        .args(args)
        .output()
        .unwrap();
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
            patchkit(&["pack", id, "--device", "dt2", "-o", out.to_str().unwrap()]);
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
