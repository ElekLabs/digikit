//! `core/card-guard` over the real Syntakt image and port (skipped when the image is absent): the two entry hooks are installed in the patched image, jump to the
//! adapter's shims, and the displaced stock instructions are in the tails; a set that has the runtime and not the guard is refused.

use patchkit::apply;
use patchkit::module::Library;
use patchkit::port::Registry;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn image() -> Option<Vec<u8>> {
    std::fs::read(root().join("firmware/Syntakt_OS1.42.syx")).ok()
}

/// A symbol of the plan by module id and name; the version of the module is not part of the lookup.
fn symbol(plan: &serde_json::Value, module: &str, name: &str) -> u64 {
    let prefix = format!("{module}@");
    plan["symbols"]
        .as_object()
        .unwrap()
        .iter()
        .find(|(k, _)| {
            k.strip_prefix(&prefix)
                .and_then(|r| r.split_once(':'))
                .is_some_and(|(_, n)| n == name)
        })
        .and_then(|(_, v)| v.as_u64())
        .unwrap_or(0)
}

fn registry() -> Registry {
    Registry::from_dir(&root().join("ports")).unwrap()
}

/// The plan of a patched image as JSON (the fields of `Plan`; a stable record of what was installed where).
fn plan_of(raw: &[u8], set: &str) -> serde_json::Value {
    let lib = Library::scan_repo_or_skip(&root()).expect("the library");
    let st = patchkit::devdata::all()
        .iter()
        .find(|d| d.short == "st")
        .unwrap();
    let (ids, _) = lib.with_requires(&st.patch_sets[set]).unwrap();
    assert!(ids.iter().any(|i| i == "core/card-guard"), "{set}");
    let out = apply::apply(raw, &registry(), &lib, &ids).unwrap();
    serde_json::to_value(&out.plan).unwrap()
}

#[test]
fn the_entry_hooks_are_installed_at_the_firmware_entries_with_their_displaced_instructions_and_tails()
 {
    let Some(raw) = image() else { return };
    if Library::scan_repo_or_skip(&root()).is_none() {
        return;
    }
    for set in ["device", "emulator"] {
        let plan = plan_of(&raw, set);
        let hooks = plan["hooks"].as_array().unwrap();
        let sym = |name: &str| symbol(&plan, "core/card-guard", name);
        let hook = |anchor: &str| {
            hooks
                .iter()
                .find(|h| h["anchor"] == anchor)
                .unwrap_or_else(|| panic!("{set}: no hook at {anchor}"))
        };
        // (site anchor, firmware entry, the shim, the displaced instructions)
        let rows = [
            (
                "cf.storage.site.block_write.entry",
                0x4010f7f8u64,
                "dkcg_write_hook",
                [
                    "4fefffec  lea.l (-0x14,a7),a7",
                    "48d7043c  movem.l {d2 d3 d4 d5 a2},(a7)",
                ],
            ),
            (
                "cf.storage.site.block_erase.entry",
                0x4010f52a,
                "dkcg_erase_hook",
                [
                    "4feffff4  lea.l (-0xc,a7),a7",
                    "48d7040c  movem.l {d2 d3 a2},(a7)",
                ],
            ),
        ];
        for (anchor, entry, shim, displaced) in rows {
            let h = hook(anchor);
            assert!(
                h["module"]
                    .as_str()
                    .unwrap()
                    .starts_with("core/card-guard@")
            );
            assert_eq!(
                h["addr"].as_u64(),
                Some(entry),
                "{set} {anchor}: the firmware entry (offset 0)"
            );
            assert_eq!(h["site_len"].as_u64(), Some(8));
            assert_eq!(
                h["ret"].as_u64(),
                Some(entry + 8),
                "{set} {anchor}: the tail returns after the eight displaced bytes"
            );
            assert_eq!(
                h["to"].as_u64(),
                Some(sym(shim)),
                "{set} {anchor}: the jump goes to {shim}"
            );
            assert_ne!(sym(shim), 0, "{shim} is placed");
            let insns: Vec<String> = h["displaced_insns"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i.as_str().unwrap().split_once("  ").unwrap().1.to_string())
                .collect();
            assert_eq!(
                insns, displaced,
                "{set} {anchor}: the displaced instructions"
            );
            assert_ne!(
                h["tail"].as_u64(),
                Some(0),
                "{set} {anchor}: the tail is placed"
            );
        }
        // the adapter's other exports and the runtime's trampoline label are placed (the object links)
        for name in [
            "dkcg_write",
            "dkcg_erase",
            "dkcg_stats",
            "dkcg_rows",
            "dkcg_row_n",
        ] {
            assert_ne!(sym(name), 0, "{set}: {name}");
        }
        let raw_ret = symbol(&plan, "core/runtime", "dkrt_raw_write_ret");
        let raw_write = symbol(&plan, "core/runtime", "dkrt_raw_block_write");
        assert!(
            raw_write != 0 && raw_ret > raw_write && raw_ret < raw_write + 32,
            "{set}: the label follows the jsr of the trampoline"
        );
        // the return address of the stock write's own recursive call (the adapter's `cg_write_split_ret`) is the instruction after the `jsr (A2)` at entry + 0x34
        let reg = registry();
        let port = reg.ports.iter().find(|p| p.port.device == "st").unwrap();
        let (entry, split) = (
            port.anchor("cf.storage.site.block_write.entry")
                .unwrap()
                .addr,
            port.anchor("cf.storage.site.block_write.split_ret")
                .unwrap()
                .addr,
        );
        assert_eq!(split, entry + 0x36, "{set}");
    }
}

#[test]
fn a_set_with_the_runtime_and_without_the_guard_is_refused_by_apply() {
    let Some(raw) = image() else { return };
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    let reg = registry();
    let st = patchkit::devdata::all()
        .iter()
        .find(|d| d.short == "st")
        .unwrap();
    let (mut ids, _) = lib.with_requires(&st.patch_sets["device"]).unwrap();
    ids.retain(|i| i != "core/card-guard");
    let e = apply::apply(&raw, &reg, &lib, &ids)
        .err()
        .expect("refused")
        .0;
    assert!(
        e.contains("includes core/runtime but not core/card-guard")
            && e.contains("card.guard.enabled"),
        "{e}"
    );
    // the same selection without the runtime is fine (the optional first flash holds neither)
    let (peer, _) = lib.with_requires(&st.patch_sets["device-peer"]).unwrap();
    apply::apply(&raw, &reg, &lib, &peer).unwrap();
}
