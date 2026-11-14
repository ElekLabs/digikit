//! `patchkit objdiff`: moves and reordering are identical; code, relocation and data changes are caught.

use patchkit::dko::{Dko, DkoReloc, DkoSection, DkoSymbol};
use patchkit::error::hex;
use patchkit::objdiff::{diff, norm_name};
use std::collections::BTreeMap;
use std::path::PathBuf;

// dkrt::rd32 and dkrt::foo::rd32 (v0 mangling, as rustc writes them)
const RD32: &str = "_RNvCsdWzhuXZnywa_4dkrt4rd32";
const RD32_MOVED: &str = "_RNvNtCsdWzhuXZnywa_4dkrt3foo4rd32";
const WR32: &str = "_RNvCsdWzhuXZnywa_4dkrt4wr32";
const WR32_MOVED: &str = "_RNvNtCsdWzhuXZnywa_4dkrt3bar4wr32";

fn text(name: &str, code: &[u8]) -> DkoSection {
    DkoSection {
        name: format!(".text.{name}"),
        kind: "text".into(),
        align: 2,
        size: code.len() as u32,
        hex: hex(code),
    }
}

fn data(name: &str, bytes: &[u8]) -> DkoSection {
    DkoSection {
        name: name.into(),
        kind: "data".into(),
        align: 1,
        size: bytes.len() as u32,
        hex: hex(bytes),
    }
}

fn reloc(section: &str, offset: u64, symbol: &str, addend: i64) -> DkoReloc {
    DkoReloc {
        section: format!(".text.{section}"),
        offset,
        rtype: "cf.32".into(),
        symbol: symbol.into(),
        addend,
    }
}

fn object(sections: Vec<DkoSection>, relocs: Vec<DkoReloc>) -> Dko {
    let mut symbols: Vec<DkoSymbol> = sections
        .iter()
        .map(|s| DkoSymbol {
            name: format!(".sec.{}", s.name),
            section: Some(s.name.clone()),
            value: 0,
            global: false,
        })
        .collect();
    symbols.push(DkoSymbol {
        name: "memcpy".into(),
        section: None,
        value: 0,
        global: true,
    });
    Dko {
        format: 1,
        arch: "cf".into(),
        source_sha256: String::new(),
        toolchain: BTreeMap::new(),
        sections,
        symbols,
        relocs,
    }
}

// moveq #1,d0; jsr <reloc>; rts
fn caller(moveq: u8) -> Vec<u8> {
    vec![0x70, moveq, 0x4e, 0xb9, 0, 0, 0, 0, 0x4e, 0x75]
}

/// rd32 and wr32 (under two module paths) and a caller of rd32 that reads a constant.
fn sample(rd: &str, wr: &str, swap: bool, moveq: u8, target: &str, byte: u8) -> Dko {
    let mut secs = vec![
        text(rd, &[0x4e, 0x71, 0x4e, 0x75]),
        text(wr, &[0x4e, 0x75]),
        text("caller", &caller(moveq)),
        data(".rodata.global.1A", &[1, 2, 3, byte]),
    ];
    if swap {
        secs.reverse();
    }
    let t = |n: &str| format!(".sec..text.{n}");
    let target = match target {
        "rd" => t(rd),
        "wr" => t(wr),
        other => other.to_string(),
    };
    object(secs, vec![reloc("caller", 4, &target, 0)])
}

#[test]
fn norm_name_strips_module_paths_and_keeps_c_symbols() {
    assert_eq!(norm_name(RD32), "rd32");
    assert_eq!(norm_name(RD32_MOVED), "rd32");
    assert_eq!(norm_name("dkrt_boot"), "dkrt_boot");
}

#[test]
fn renamed_module_path_is_identical() {
    let a = sample(RD32, WR32, false, 1, "rd", 4);
    let b = sample(RD32_MOVED, WR32_MOVED, false, 1, "rd", 4);
    let r = diff(&a, &b).unwrap();
    assert!(r.identical, "{}", r.text());
    assert_eq!((r.functions, r.data_objects), (3, 1));
    assert_eq!(r.text(), "identical: 3 functions, 1 data objects\n");
}

#[test]
fn reordered_functions_are_identical() {
    let a = sample(RD32, WR32, false, 1, "rd", 4);
    let b = sample(RD32_MOVED, WR32_MOVED, true, 1, "rd", 4);
    assert!(diff(&a, &b).unwrap().identical);
}

#[test]
fn one_instruction_change_is_caught() {
    let a = sample(RD32, WR32, false, 1, "rd", 4);
    let b = sample(RD32, WR32, false, 2, "rd", 4);
    let r = diff(&a, &b).unwrap();
    assert!(!r.identical);
    assert_eq!(r.differences.len(), 1);
    let d = &r.differences[0];
    assert_eq!((d.kind.as_str(), d.name.as_str()), ("differs", "caller"));
    assert!(
        d.detail.contains("first difference at +0x1"),
        "{}",
        d.detail
    );
    assert!(d.detail.contains("- +0x0000  7001"), "{}", d.detail);
    assert!(d.detail.contains("+ +0x0000  7002"), "{}", d.detail);
}

#[test]
fn changed_relocation_target_is_caught() {
    let a = sample(RD32, WR32, false, 1, "rd", 4);
    let b = sample(RD32, WR32, false, 1, "wr", 4);
    let r = diff(&a, &b).unwrap();
    assert_eq!(r.differences.len(), 1, "{}", r.text());
    assert!(r.differences[0].detail.contains("relocation"));
    assert!(r.differences[0].detail.contains("reloc +0x2 .text.rd32+0"));
    assert!(r.differences[0].detail.contains("reloc +0x2 .text.wr32+0"));
    // another symbol, and another addend
    let c = sample(RD32, WR32, false, 1, "memcpy", 4);
    assert!(!diff(&a, &c).unwrap().identical);
}

#[test]
fn changed_data_byte_is_caught() {
    let a = sample(RD32, WR32, false, 1, "rd", 4);
    let b = sample(RD32, WR32, false, 1, "rd", 5);
    let r = diff(&a, &b).unwrap();
    assert!(!r.identical);
    assert!(
        r.differences.iter().any(|d| d.kind == "only-old"),
        "{}",
        r.text()
    );
}

#[test]
fn named_data_changed_byte_is_caught_and_c_symbol_change_is_flagged() {
    let mk =
        |name: &str, byte: u8| object(vec![data(&format!(".data.{name}"), &[7, byte])], vec![]);
    let r = diff(&mk("DKRT_TABLE", 1), &mk("DKRT_TABLE", 2)).unwrap();
    assert_eq!(r.differences.len(), 1);
    assert!(r.differences[0].detail.contains("first difference at +0x1"));
    let r = diff(&mk("DKRT_TABLE", 1), &mk("DKRT_OTHER", 1)).unwrap();
    assert!(
        r.differences.iter().all(|d| d.kind == "c-symbol"),
        "{}",
        r.text()
    );
    assert_eq!(r.differences.len(), 2);
}

#[test]
fn removed_function_and_extern_are_reported() {
    let a = sample(RD32, WR32, false, 1, "rd", 4);
    let mut b = sample(RD32, WR32, false, 1, "rd", 4);
    b.sections.retain(|s| s.name != format!(".text.{WR32}"));
    b.symbols.retain(|s| s.name != "memcpy");
    let r = diff(&a, &b).unwrap();
    let kinds: Vec<_> = r
        .differences
        .iter()
        .map(|d| (d.kind.as_str(), d.name.as_str()))
        .collect();
    assert!(kinds.contains(&("only-old", "wr32")), "{kinds:?}");
    assert!(kinds.contains(&("extern", "memcpy")), "{kinds:?}");
}

fn runtime_object() -> Dko {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../modules/core/runtime/build/runtime.dko.json");
    Dko::parse(&std::fs::read_to_string(p).unwrap()).unwrap()
}

#[test]
fn tracked_runtime_object_equals_itself() {
    let o = runtime_object();
    let r = diff(&o, &o).unwrap();
    assert!(r.identical, "{}", r.text());
    assert!(r.functions > 500 && r.data_objects > 100);
}
