//! Conflict and safety rules of `apply`, with synthetic modules over the real ports and stock images
//! (skipped when the images are absent).

use patchkit::apply::{self, Options};
use patchkit::firmware::Firmware;
use patchkit::module::{Library, Module};
use patchkit::port::{Hook, Registry};
use serde_json::json;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn image(name: &str) -> Option<Vec<u8>> {
    std::fs::read(root().join("firmware").join(name)).ok()
}

const DT2: &str = "Digitakt_II_OS1.17.syx";

/// The content modules are built on demand (`Library::scan_repo`): a test that needs one skips without the toolchain.
macro_rules! need_toolchain {
    () => {
        if !patchkit::build::toolchain::Env::new(&root()).m68k_or_skip() {
            return;
        }
    };
}

fn registry() -> Registry {
    Registry::from_dir(&root().join("devices/ports")).unwrap()
}

/// The real library (core/expansion, core/fx, ...) plus synthetic modules.
fn library(extra: Vec<Module>) -> Library {
    // without the toolchain the content modules are left out; the tests that need one start with `need_toolchain!()`
    let mut lib = Library::scan_repo_or_skip(&root())
        .unwrap_or_else(|| Library::scan_repo_for(&root(), true, Some(&[])).unwrap());
    lib.modules.extend(extra);
    lib
}

/// A module whose manifest is `body` after a standard [module] header; `dko` is its only object, if any.
fn module(id: &str, header_extra: &str, body: &str, dko: Option<serde_json::Value>) -> Module {
    let text = format!(
        "[module]\nid = \"{id}\"\nversion = \"1.0.0\"\nname = \"{id}\"\ncategory = \"test\"\ntype = \"raw\"\nauthor = \"digikit\"\nstatus = \"experimental\"\nsummary = \"synthetic\"\n{header_extra}\n{body}\n"
    );
    let read = move |_p: &str| -> patchkit::Result<String> { Ok(dko.clone().unwrap().to_string()) };
    Module::from_parts(&text, &read).unwrap()
}

fn write_op(offset: i64, hex: &str) -> String {
    format!(
        "[[op]]\nop = \"write\"\nanchor = \"dsp.master.fn.q31_converter\"\noffset = {offset}\nhex = \"{hex}\"\n"
    )
}

fn expect_err(r: patchkit::Result<apply::Applied>, needle: &str) {
    match r {
        Ok(_) => panic!("expected an error containing {needle:?}"),
        Err(e) => assert!(
            e.0.contains(needle),
            "error {:?} does not contain {needle:?}",
            e.0
        ),
    }
}

fn run(lib: &Library, ids: &[&str]) -> patchkit::Result<apply::Applied> {
    run_with(&registry(), lib, ids)
}

fn run_with(reg: &Registry, lib: &Library, ids: &[&str]) -> patchkit::Result<apply::Applied> {
    let raw = image(DT2).expect("image");
    let ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
    apply::apply_with(&raw, reg, lib, &ids, Options::default())
}

macro_rules! need_image {
    () => {
        if image(DT2).is_none() {
            return;
        }
    };
}

#[test]
fn unmet_requires_and_conflicts_are_refused() {
    need_image!();
    let a = module("t/a", "requires = [\"t/b\"]", "", None);
    let b = module("t/b", "", "", None);
    let c = module("t/c", "conflicts = [\"t/b\"]", "", None);
    let lib = library(vec![a, b, c]);
    expect_err(run(&lib, &["t/a"]), "requires t/b");
    expect_err(run(&lib, &["t/c", "t/b"]), "conflicts with");
    expect_err(run(&lib, &["t/missing"]), "unknown module");
    assert!(run(&lib, &["t/a", "t/b"]).is_ok());
}

#[test]
fn order_is_canonical_whatever_the_selection_order() {
    need_image!();
    let a = module("t/a", "", "", None);
    let b = module("t/b", "requires = [\"t/a\"]", "", None);
    let c = module("t/c", "", "", None);
    let lib = library(vec![a, b, c]);
    let x = run(&lib, &["t/c", "t/b", "t/a"]).unwrap();
    let y = run(&lib, &["t/a", "t/c", "t/b"]).unwrap();
    assert_eq!(x.plan.modules, vec!["t/a@1.0.0", "t/b@1.0.0", "t/c@1.0.0"]);
    assert!(x.syx == y.syx, "different selection order, different bytes");
}

#[test]
fn colliding_claims_are_refused() {
    need_image!();
    let claim = "[[claim]]\nresource = \"dsp.thing\"\n";
    let lib = library(vec![
        module("t/a", "", claim, None),
        module("t/b", "", claim, None),
    ]);
    expect_err(run(&lib, &["t/a", "t/b"]), "both claim dsp.thing");
}

#[test]
fn overlapping_writes_are_refused() {
    need_image!();
    // both rewrite the converter's first instruction word (6 bytes at the anchor); expect_sha256 is not
    // needed inside the anchor
    let lib = library(vec![
        module("t/a", "", &write_op(0, "180400bd0000"), None),
        module("t/b", "", &write_op(2, "00bd0000"), None),
    ]);
    expect_err(run(&lib, &["t/a", "t/b"]), "overlapping writes");
}

#[test]
fn a_write_outside_its_anchor_needs_a_pinned_old_value() {
    need_image!();
    let lib = library(vec![module("t/a", "", &write_op(0x100, "0000"), None)]);
    expect_err(run(&lib, &["t/a"]), "no expect_sha256");
    let wrong = format!(
        "{}expect_sha256 = \"{}\"\n",
        write_op(0x100, "0000"),
        "00".repeat(32)
    );
    let lib = library(vec![module("t/a", "", &wrong, None)]);
    expect_err(run(&lib, &["t/a"]), "old bytes hash");
}

#[test]
fn a_region_over_capacity_is_refused() {
    need_image!();
    let dko = json!({
        "format": 1, "arch": "cf", "source_sha256": "0".repeat(64),
        "sections": [{"name": ".bss", "kind": "bss", "align": 4, "size": 3_000_000, "hex": ""}],
        "symbols": [], "relocs": []
    });
    let m = module(
        "t/big",
        "",
        "[[object]]\npath = \"x.json\"\nplace = \"cf.expansion\"\n",
        Some(dko),
    );
    let lib = library(vec![m]);
    expect_err(run(&lib, &["t/big"]), "do not fit in cf.expansion");
}

#[test]
fn an_anchor_hash_mismatch_is_refused() {
    need_image!();
    need_toolchain!();
    let mut reg = registry();
    for p in reg
        .ports
        .iter_mut()
        .filter(|p| p.port.version == "1.17" && p.port.device == "dt2")
    {
        for a in p.anchor.iter_mut().filter(|a| a.name == "dsp.fx.fn.entry") {
            a.sha256 = "ab".repeat(32);
        }
    }
    let lib = library(vec![]);
    expect_err(
        run_with(&reg, &lib, &["core/dsp-link", "core/fx"]),
        "bytes hash",
    );
}

#[test]
fn a_module_for_another_firmware_is_refused() {
    need_image!();
    let m = module("t/pinned", "ports = [\"dn2-1.12\"]", "", None);
    let lib = library(vec![m]);
    expect_err(run(&lib, &["t/pinned"]), "not compatible");
}

#[test]
fn the_container_cap_is_enforced() {
    need_image!();
    need_toolchain!();
    let mut reg = registry();
    for p in reg
        .ports
        .iter_mut()
        .filter(|p| p.port.version == "1.17" && p.port.device == "dt2")
    {
        p.limits.container_max = 1_000_000;
    }
    let lib = library(vec![]);
    expect_err(
        run_with(&reg, &lib, &["core/dsp-link", "core/fx"]),
        "over the",
    );
}

/// The emulator set of every device file, on every stock image in `firmware/` that a port knows: without the flash cap it always
/// applies; with the cap, an image over the device's `container_max` is refused and names the limit.
#[test]
fn the_emulator_set_applies_without_the_flash_cap() {
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    let reg = registry();
    let mut checked = 0;
    for dev in patchkit::devdata::all() {
        let Some(set) = dev.patch_sets.get("emulator") else {
            continue;
        };
        // The device's current OS: the acceptance firmware, else the highest port version.
        let key = |v: &str| -> Vec<u64> { v.split('.').filter_map(|n| n.parse().ok()).collect() };
        let Some(current) = dev
            .identity
            .acceptance
            .as_ref()
            .map(|a| a.firmware.clone())
            .or_else(|| {
                reg.ports
                    .iter()
                    .filter(|p| p.port.device == dev.short)
                    .map(|p| p.port.version.clone())
                    .max_by_key(|v| key(v))
            })
        else {
            continue;
        };
        let Ok(files) = std::fs::read_dir(root().join("firmware")) else {
            continue;
        };
        let mut names: Vec<_> = files.flatten().map(|e| e.path()).collect();
        names.sort();
        for path in names
            .iter()
            .filter(|p| p.extension().is_some_and(|e| e == "syx"))
        {
            let Ok(raw) = std::fs::read(path) else {
                continue;
            };
            let Ok(fw) = Firmware::parse(&raw) else {
                continue;
            };
            if fw.device.short() != dev.short {
                continue;
            }
            let Some(port) = reg.find(&fw.syx_sha256) else {
                continue;
            };
            if port.port.version != current {
                continue;
            }
            let (ids, _) = lib.with_requires(set).unwrap();
            let name = path.display();
            let open = Options {
                flash_cap: false,
                ..Options::default()
            };
            let out = apply::apply_with(&raw, &reg, &lib, &ids, open).unwrap_or_else(|e| {
                panic!("{name}: the emulator set must apply without the cap: {e}")
            });
            let bytes = patchkit::verify::verify(&out.syx, Some(&reg), Some(&raw))
                .unwrap_or_else(|e| panic!("{name}: verify refuses the emulator image: {e}"))
                .container_bytes;
            let capped = apply::apply_with(&raw, &reg, &lib, &ids, Options::default());
            if bytes > dev.container_max {
                expect_err(
                    capped,
                    &format!("over the {}-byte limit", dev.container_max),
                );
            } else {
                assert!(
                    capped.is_ok(),
                    "{name}: under the cap, so the cap must not refuse"
                );
            }
            checked += 1;
        }
    }
    if checked == 0 {
        eprintln!(
            "SKIPPED the_emulator_set_applies_without_the_flash_cap: no stock image of a device with an emulator set in firmware/"
        );
    }
}

#[test]
fn sections_2_4_and_8_can_never_change() {
    need_image!();
    let mut fw = Firmware::parse(&image(DT2).unwrap()).unwrap();
    for id in [2u32, 4, 8] {
        assert!(fw.replace(id, vec![0; 16]).is_err(), "section {id}");
    }
    assert!(fw.replace(7, fw.section(7).unwrap().data.clone()).is_ok());
}

#[test]
fn a_patched_image_has_no_port() {
    need_image!();
    need_toolchain!();
    let out = run(&library(vec![]), &["core/dsp-link", "core/fx"]).unwrap();
    let lib = library(vec![]);
    let again = apply::apply(
        &out.syx,
        &registry(),
        &lib,
        &["core/dsp-link".to_string(), "core/fx".to_string()],
    );
    expect_err(again, "no port for firmware");
}

#[test]
fn retarget_of_something_that_is_not_a_call_is_refused() {
    need_image!();
    let m = module(
        "t/r",
        "",
        "[[op]]\nop = \"hook\"\nhook = \"t.retarget.hook\"\nto = \"_x\"\n[[object]]\npath = \"x.json\"\nplace = \"dsp.l2\"\n",
        Some(json!({
            "format": 1, "arch": "sharc", "source_sha256": "0".repeat(64),
            "sections": [{"name": "seg_pmco", "kind": "text", "align": 8, "size": 6, "hex": "000000000000"}],
            "symbols": [{"name": "_x", "section": "seg_pmco", "value": 0, "global": true}],
            "relocs": []
        })),
    );
    let lib = library(vec![m]);
    let mut reg = registry();
    let sha = Firmware::parse(&image(DT2).unwrap()).unwrap().syx_sha256;
    for p in reg.ports.iter_mut().filter(|p| p.port.syx_sha256 == sha) {
        p.hook.push(Hook {
            name: "t.retarget.hook".into(),
            site: "dsp.master.fn.q31_converter".into(),
            offset: 0,
            mode: "retarget".into(),
            clobbers: vec![],
            len: None,
            kind: None,
            check: None,
            fixups: vec![],
        });
    }
    expect_err(run_with(&reg, &lib, &["t/r"]), "not a direct CALL");
}

// ---- optional_anchors: an anchor or hook that the port lacks is not a reason to refuse the module ----

/// An object with a marker, then two words that name undefined symbols: an anchor the port lacks and the tail of a hook site it lacks.
fn dko() -> serde_json::Value {
    json!({
        "format": 1, "arch": "cf", "source_sha256": "0".repeat(64),
        "sections": [{"name": ".data", "kind": "data", "align": 4, "size": 16, "hex": "4f50544d41524b310000000100000002"}],
        "symbols": [
            {"name": "cf.no.such.anchor", "section": null, "value": 0, "global": true},
            {"name": "tail", "section": null, "value": 0, "global": true},
            {"name": "marker", "section": ".data", "value": 0, "global": true}
        ],
        "relocs": [
            {"section": ".data", "offset": 8, "type": "cf.32", "symbol": "cf.no.such.anchor", "addend": 0},
            {"section": ".data", "offset": 12, "type": "cf.32", "symbol": "tail", "addend": 0}
        ]
    })
}

fn optional_module(id: &str, optional: &[&str], op: &str) -> Module {
    let list: Vec<String> = optional.iter().map(|s| format!("{s:?}")).collect();
    module(
        id,
        &format!("optional_anchors = [{}]", list.join(", ")),
        &format!(
            "[[object]]\npath = \"x.json\"\nplace = \"cf.expansion\"\n[object.externs]\ntail = \"hook.cf.no.site.tail\"\n\n{op}"
        ),
        Some(dko()),
    )
}

fn run_one(m: Module) -> patchkit::Result<apply::Applied> {
    let id = m.id().to_string();
    run(&library(vec![m]), &[id.as_str()])
}

#[test]
fn a_missing_anchor_without_the_optional_mark_is_refused() {
    need_image!();
    let r = run_one(optional_module("t/strict", &[], ""));
    let e = r.err().expect("refused").0;
    assert!(
        e.contains("not compatible") && e.contains("cf.no.such.anchor"),
        "{e}"
    );
}

#[test]
fn a_missing_optional_anchor_links_to_zero_and_is_listed() {
    need_image!();
    let r = run_one(optional_module(
        "t/opt",
        &["cf.no.such.anchor", "cf.no.site"],
        "",
    ));
    let a = r.unwrap();
    assert_eq!(
        a.plan.disabled,
        ["t/opt@1.0.0:cf.no.such.anchor", "t/opt@1.0.0:cf.no.site"]
    );
    // the marker (ASCII "OPTMARK1"), then the two links: both are 0 in the image
    let fw = patchkit::firmware::Firmware::parse(&a.syx).unwrap();
    let main = &fw.main().unwrap().data;
    let at = main
        .windows(8)
        .position(|w| w == b"OPTMARK1")
        .expect("the object is in the image");
    assert_eq!(&main[at + 8..at + 16], [0u8; 8]);
}

#[test]
fn an_optional_name_the_port_has_is_not_disabled() {
    // the anchor of the stock converter exists on this port: it stays enabled; the missing one is still refused
    let r = run_one(optional_module(
        "t/mixed",
        &["dsp.master.fn.q31_converter"],
        "",
    ));
    let e = r.err().expect("refused").0;
    assert!(e.contains("cf.no.such.anchor"), "{e}");
}

#[test]
fn a_hook_op_that_the_port_lacks_is_skipped_when_it_is_optional() {
    need_image!();
    let op = "[[op]]\nop = \"hook\"\nhook = \"cf.no.such.hook\"\nto = \"marker\"\n";
    let r = run_one(optional_module(
        "t/nohook",
        &["cf.no.such.anchor", "cf.no.site", "cf.no.such.hook"],
        op,
    ));
    let a = r.unwrap();
    assert!(a.plan.hooks.is_empty());
    assert!(
        a.plan
            .disabled
            .iter()
            .any(|d| d.ends_with(":cf.no.such.hook")),
        "{:?}",
        a.plan.disabled
    );
    // the same hook op without the mark is refused
    let r = run_one(optional_module(
        "t/hard",
        &["cf.no.such.anchor", "cf.no.site"],
        op,
    ));
    let e = r.err().expect("refused").0;
    assert!(e.contains("hook cf.no.such.hook"), "{e}");
}
