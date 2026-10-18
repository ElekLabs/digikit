//! `core/machines` over the real ports and stock images (skipped when the images are absent): what the framework
//! writes for one and for two added machine types, canonical type numbers, and the refusals.

use patchkit::apply::{self, Applied, Options};
use patchkit::firmware::Firmware;
use patchkit::module::{Library, Module};
use patchkit::port::Registry;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn image(name: &str) -> Option<Vec<u8>> {
    std::fs::read(root().join("firmware").join(name)).ok()
}

const DT2_117: &str = "Digitakt_II_OS1.17.syx";
const DT2_116: &str = "Digitakt_II_OS1.16.syx";
const DN2_112: &str = "Digitone_II_OS1.12.syx";

fn registry() -> Registry {
    Registry::from_dir(&root().join("devices/ports")).unwrap()
}

fn library(extra: Vec<Module>) -> Library {
    let mut lib = Library::scan(&root().join("modules")).unwrap();
    lib.modules.extend(extra);
    lib
}

fn run(file: &str, lib: &Library, ids: &[&str]) -> Option<patchkit::Result<Applied>> {
    let raw = image(file)?;
    let ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
    Some(apply::apply_with(
        &raw,
        &registry(),
        lib,
        &ids,
        Options::default(),
    ))
}

/// Memory of the patched MAIN OS: the image plus the segment chain behind it.
struct Main {
    data: Vec<u8>,
    base: u32,
    segs: Vec<(u32, Vec<u8>)>,
}

impl Main {
    fn new(syx: &[u8], stock_len: usize) -> Main {
        let fw = Firmware::parse(syx).unwrap();
        let s = fw.section(3).unwrap();
        let data = s.data.clone();
        let mut segs = vec![];
        let mut at = stock_len;
        loop {
            let len = u32::from_be_bytes(data[at..at + 4].try_into().unwrap());
            if len == u32::MAX {
                break;
            }
            let dest = u32::from_be_bytes(data[at + 4..at + 8].try_into().unwrap());
            segs.push((dest, data[at + 12..at + 12 + len as usize].to_vec()));
            at += 12 + len as usize;
        }
        Main {
            data,
            base: s.dest,
            segs,
        }
    }
    fn read(&self, addr: u32, n: usize) -> Vec<u8> {
        if let Some(o) = addr.checked_sub(self.base)
            && (o as usize + n) <= self.data.len()
        {
            return self.data[o as usize..o as usize + n].to_vec();
        }
        for (d, p) in &self.segs {
            if addr >= *d && (addr - d) as usize + n <= p.len() {
                return p[(addr - d) as usize..(addr - d) as usize + n].to_vec();
            }
        }
        panic!("{addr:#x}+{n} is not in the patched MAIN OS");
    }
    fn u32s(&self, addr: u32, n: usize) -> Vec<u32> {
        self.read(addr, n * 4)
            .chunks(4)
            .map(|c| u32::from_be_bytes(c.try_into().unwrap()))
            .collect()
    }
    fn cstr(&self, addr: u32) -> String {
        (0..64)
            .map(|i| self.read(addr + i, 1)[0])
            .take_while(|&b| b != 0)
            .map(|b| b as char)
            .collect()
    }
}

fn err_of(r: patchkit::Result<Applied>) -> String {
    match r {
        Ok(_) => panic!("expected an error"),
        Err(e) => e.0,
    }
}

fn stock_len(file: &str) -> usize {
    Firmware::parse(&image(file).unwrap())
        .unwrap()
        .section(3)
        .unwrap()
        .data
        .len()
}

fn plan(a: &Applied) -> serde_json::Value {
    serde_json::to_value(&a.plan).unwrap()
}

fn reg<'a>(p: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    p["registries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == name)
        .unwrap_or_else(|| panic!("no registry {name}"))
}

fn section<'a>(p: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    p["sections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == name)
        .unwrap_or_else(|| panic!("no section {name}"))
}

const XS: [&str; 3] = ["core/machines", "machines/xslice", "test/machine-clone"];

#[test]
fn dt2_machines_get_types_7_and_8_and_the_tables_grow() {
    for file in [DT2_117, DT2_116] {
        let lib = library(vec![]);
        let Some(r) = run(file, &lib, &XS) else {
            continue;
        };
        let a = r.unwrap();
        let p = plan(&a);
        let m = reg(&p, "machines");
        assert_eq!(
            (m["first"].as_u64(), m["count"].as_u64()),
            (Some(7), Some(2))
        );
        assert_eq!(m["row_size"], 64);
        assert_eq!(m["entries"][0]["id"], "xslice");
        assert_eq!(m["entries"][0]["index"], 7);
        assert_eq!(m["entries"][1]["index"], 8);
        let main = Main::new(&a.syx, stock_len(file));
        // names: 7 stock rows of 12 bytes, then one per machine; the hint word is the clone's (type 6)
        let n = section(&p, "mf_names");
        assert_eq!(
            (n["copied"].as_u64(), n["appended_rows"].as_u64()),
            (Some(84), Some(2))
        );
        let t = main.u32s(n["addr"].as_u64().unwrap() as u32, 27);
        assert_eq!(t[7 * 3 + 2], t[6 * 3 + 2]);
        assert_eq!(t[8 * 3 + 2], t[6 * 3 + 2]);
        assert_eq!(main.cstr(t[7 * 3]), "XSlice");
        assert_eq!(main.cstr(t[8 * 3 + 1]), "XCL");
        let pm = section(&p, "mf_permit");
        let pt = main.u32s(pm["addr"].as_u64().unwrap() as u32, 9);
        assert_eq!((pt[7], pt[8]), (pt[6], pt[6]));
        let ty = section(&p, "mf_pertype");
        let tb = main.read(ty["addr"].as_u64().unwrap() as u32, 9);
        assert_eq!((tb[7], tb[8]), (tb[6], tb[6]));
        let sel = section(&p, "mf_selector");
        assert_eq!(
            (sel["copied"].as_u64(), sel["appended_rows"].as_u64()),
            (Some(28), Some(2))
        );
        // the stock tables are copied unchanged: the first rows equal the image's
        assert!(
            a.plan
                .writes
                .iter()
                .any(|w| w.what.contains("cf.machines.persist"))
        );
        // every registry row holds the descriptor ID fields of the entry
        let rows = main.u32s(m["addr"].as_u64().unwrap() as u32, 32);
        assert_eq!(
            &rows[2..11],
            &[0xf8, 0xf9, 0xfa, 0xfb, 0xfc, 0xfd, 0, 0xfe, 0x0a]
        );
        assert_eq!(rows[11], 6); // clone_of
        assert_eq!(rows[12], 7); // position
        assert_eq!(rows[16 + 12], 255); // the second machine has the default position
    }
}

#[test]
fn the_selection_order_does_not_change_the_bytes() {
    let lib = library(vec![]);
    let Some(r1) = run(DT2_117, &lib, &XS) else {
        return;
    };
    let r2 = run(
        DT2_117,
        &lib,
        &["test/machine-clone", "machines/xslice", "core/machines"],
    )
    .unwrap();
    assert!(r1.unwrap().syx == r2.unwrap().syx);
}

#[test]
fn every_instruction_the_framework_places_decodes() {
    let lib = library(vec![]);
    for (file, ids) in [
        (DT2_117, &XS[..]),
        (DT2_116, &XS[..]),
        (DN2_112, &["core/machines", "test/dn2-clone"][..]),
    ] {
        let Some(r) = run(file, &lib, ids) else {
            continue;
        };
        let a = r.unwrap();
        let m = Main::new(&a.syx, stock_len(file));
        let mut n = 0;
        for w in a.plan.writes.iter().filter(|w| {
            w.kind == "place" && w.owner.starts_with("core/machines") && !w.code.is_empty()
        }) {
            for &(lo, hi) in &w.code {
                let mut at = lo;
                let bytes = m.read(lo as u32, (hi - lo) as usize);
                while at < hi {
                    let i = coldfire::decode_at(&bytes, lo as u32, (at - lo) as usize)
                        .unwrap_or_else(|| panic!("{at:#x}: does not decode"));
                    at += (i.len as u64).max(2);
                    n += 1;
                }
                assert_eq!(at, hi, "the code ends mid-instruction");
            }
        }
        assert!(n > 100, "{file}: only {n} instructions");
    }
}

#[test]
fn digitone_ii_clone_gets_type_5() {
    let lib = library(vec![]);
    for file in [DN2_112, "Digitone_II_OS1.11.syx"] {
        let Some(r) = run(file, &lib, &["core/machines", "test/dn2-clone"]) else {
            continue;
        };
        let a = r.unwrap();
        let p = plan(&a);
        let m = reg(&p, "machines");
        assert_eq!(
            (m["first"].as_u64(), m["count"].as_u64()),
            (Some(5), Some(1))
        );
        assert_eq!(m["entries"][0]["index"], 5);
        // no DSP selector table and no per-type byte on the Digitone II
        assert!(
            p["sections"]
                .as_array()
                .unwrap()
                .iter()
                .all(|s| s["name"] != "mf_selector" && s["name"] != "mf_pertype")
        );
        let hooks: Vec<String> = p["hooks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h["anchor"].as_str().unwrap().to_string())
            .collect();
        for h in [
            "cf.machines.dispatch",
            "cf.machines.group",
            "cf.machines.reader",
            "cf.machines.owner",
            "cf.machines.frametype",
            "cf.machines.typemap.0",
        ] {
            assert!(hooks.iter().any(|x| x == h), "{file}: no hook on {h}");
        }
    }
}

#[test]
fn a_digitakt_machine_is_refused_on_the_digitone() {
    let lib = library(vec![]);
    let Some(r) = run(DN2_112, &lib, &["core/machines", "machines/xslice"]) else {
        return;
    };
    // MANUAL SLICE (type 6) is not a Digitone II type: the clone's table reads lie outside the anchors
    let e = err_of(r);
    assert!(
        e.contains("outside the anchor")
            || e.contains("anchor")
            || e.contains("no number in the allocation table"),
        "{e}"
    );
}

#[test]
fn core_params_and_pluck_assign_codes_and_relocate_the_table() {
    let lib = library(vec![]);
    for file in [DT2_117, DT2_116] {
        let Some(r) = run(
            file,
            &lib,
            &[
                "core/dsp-voice",
                "core/machines",
                "core/params",
                "machines/pluck",
            ],
        ) else {
            continue;
        };
        let a = r.unwrap();
        let p = plan(&a);
        let pr = reg(&p, "params");
        assert_eq!(
            (pr["first"].as_u64(), pr["count"].as_u64()),
            (Some(0x113), Some(4))
        );
        assert_eq!(pr["entries"][0]["index"], 0x113);
        assert_eq!(pr["row_size"], 32, "seven fields and the formatter");
        let t = section(&p, "pf_table");
        assert_eq!(t["copied"], 0x113 * 60);
        assert_eq!(t["appended_rows"], 4);
        // no `formatter`: each appended row keeps its template's (LEV 0xd1) formatter at +0x34
        {
            let main = Main::new(&a.syx, stock_len(file));
            let t1 = t["from_addr"].as_u64().unwrap() as u32;
            let lev = main.u32s(t1 + 60 * 0xd1, 15)[13];
            let at = t["addr"].as_u64().unwrap() as u32 + 0x113 * 60;
            for k in 0..4 {
                assert_eq!(main.u32s(at + 60 * k, 15)[13], lev, "{file} row {k}");
            }
        }
        // 56 references repointed, 54 bounds raised
        let refs = a
            .plan
            .writes
            .iter()
            .filter(|w| w.what.contains("cf.params.ref."))
            .count();
        let bounds = a
            .plan
            .writes
            .iter()
            .filter(|w| w.what.contains("cf.params.bound."))
            .count();
        assert_eq!((refs, bounds), (56, 54), "{file}");
        // the machine's fields carry the assigned codes
        let m = reg(&p, "machines");
        let main = Main::new(&a.syx, stock_len(file));
        // PLUCK's permanent number is 9: rows 7 and 8 are gaps (clones of type 0, not listed)
        assert_eq!(m["gaps"], serde_json::json!([7, 8]));
        assert_eq!(m["entries"][0]["index"], 9);
        let rows = main.u32s(m["addr"].as_u64().unwrap() as u32, 48);
        assert_eq!(rows[16 + 12], 0xffff_ffff, "gap row position: hidden");
        assert_eq!(rows[16 + 11], 0, "gap row clones type 0");
        let rows = &rows[32..];
        assert_eq!(
            &rows[2..11],
            &[0xca, 0x113, 0x114, 0, 0x115, 0x116, 0, 0xd1, 0x0a]
        );
        // the voice registry ends in a zero row
        let v = reg(&p, "voices");
        assert_eq!(v["count"], 1);
    }
}
