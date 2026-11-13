//! `optional_anchors` of a manifest: an anchor or hook that the port lacks is not a reason to refuse the module; the link is 0, the hook op is
//! skipped, and the plan lists it under `disabled`. Synthetic modules over the real DT2 port and stock image (skipped when the image is absent).

use patchkit::apply::{self, Options};
use patchkit::module::{Library, Module};
use patchkit::port::Registry;
use serde_json::json;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

const IMAGE: &str = "Digitakt_II_OS1.17.syx";

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

fn module(id: &str, optional: &[&str], body: &str) -> Module {
    let list: Vec<String> = optional.iter().map(|s| format!("{s:?}")).collect();
    let text = format!(
        "[module]\nid = \"{id}\"\nversion = \"1.0.0\"\nname = \"{id}\"\ncategory = \"test\"\ntype = \"raw\"\nauthor = \"digikit\"\nstatus = \"experimental\"\nsummary = \"synthetic\"\noptional_anchors = [{}]\n\n[[object]]\npath = \"x.json\"\nplace = \"cf.expansion\"\n[object.externs]\ntail = \"hook.cf.no.site.tail\"\n\n{body}\n",
        list.join(", ")
    );
    let d = dko().to_string();
    Module::from_parts(&text, &move |_p: &str| -> patchkit::Result<String> {
        Ok(d.clone())
    })
    .unwrap()
}

fn run(m: Module) -> Option<patchkit::Result<apply::Applied>> {
    let raw = std::fs::read(root().join("firmware").join(IMAGE)).ok()?;
    let reg = Registry::from_dir(&root().join("devices/ports")).unwrap();
    let mut lib = Library::scan_repo_or_skip(&root())
        .unwrap_or_else(|| Library::scan_repo_for(&root(), true, Some(&[])).unwrap());
    let id = m.id().to_string();
    lib.modules.push(m);
    Some(apply::apply_with(
        &raw,
        &reg,
        &lib,
        &[id],
        Options::default(),
    ))
}

#[test]
fn a_missing_anchor_without_the_optional_mark_is_refused() {
    let Some(r) = run(module("t/strict", &[], "")) else {
        return;
    };
    let e = r.err().expect("refused").0;
    assert!(
        e.contains("not compatible") && e.contains("cf.no.such.anchor"),
        "{e}"
    );
}

#[test]
fn a_missing_optional_anchor_links_to_zero_and_is_listed() {
    let Some(r) = run(module("t/opt", &["cf.no.such.anchor", "cf.no.site"], "")) else {
        return;
    };
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
    let Some(r) = run(module("t/mixed", &["dsp.master.fn.q31_converter"], "")) else {
        return;
    };
    let e = r.err().expect("refused").0;
    assert!(e.contains("cf.no.such.anchor"), "{e}");
}

#[test]
fn a_hook_op_that_the_port_lacks_is_skipped_when_it_is_optional() {
    let op = "[[op]]\nop = \"hook\"\nhook = \"cf.no.such.hook\"\nto = \"marker\"\n";
    let Some(r) = run(module(
        "t/nohook",
        &["cf.no.such.anchor", "cf.no.site", "cf.no.such.hook"],
        op,
    )) else {
        return;
    };
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
    let Some(r) = run(module("t/hard", &["cf.no.such.anchor", "cf.no.site"], op)) else {
        return;
    };
    let e = r.err().expect("refused").0;
    assert!(e.contains("hook cf.no.such.hook"), "{e}");
}
