//! Address map output (docs/ADDRMAP.md): the entries of the ship map and the module map, and the `patchkit addr` command that names addresses.
//!
//! `patchkit apply --addrmap FILE` writes layers `our` (the ColdFire code of the runtime, adapters and apps the plan placed) and `anchor`
//! (the code anchors of the port). `patchkit pack --addrmap FILE` writes the module map: the code symbols of one module, link-relative.
//! SHARC code of our own is not in the map yet (docs/plans/profiling.md item 3).

use crate::dko::Dko;
use crate::error::*;
use crate::link::Linked;
use crate::port::Port;
use dkaddrmap::{AddrMap, Cpu, Entry, Key, Layer, LoadTable, Resolver};
use std::collections::BTreeMap;

/// A readable name: Rust symbols (legacy and v0 mangling) are demangled without their hash, other names stay as the linker has them.
fn readable(name: &str) -> String {
    // a symbol the linker made from a section name: `.sec..text.put_word`
    let name = name.strip_prefix(".sec.").unwrap_or(name);
    let name = name.strip_prefix(".text.").unwrap_or(name);
    if name.starts_with("_R") || name.starts_with("_ZN") {
        format!("{:#}", rustc_demangle::demangle(name))
    } else {
        name.to_string()
    }
}

/// The functions of a linked ColdFire object: every symbol that lies in an executable section. The size runs to the next symbol of the
/// section (the object has no symbol sizes; with one function per section it is the function's size).
/// With `module` the addresses are those of the link (base 0): offsets from the module's load address.
pub fn text_entries(obj: &Dko, l: &Linked, module: Option<&str>) -> Vec<Entry> {
    let sections: BTreeMap<&str, (&str, u64)> = obj
        .sections
        .iter()
        .map(|s| (s.name.as_str(), (s.kind.as_str(), s.size as u64)))
        .collect();
    let mut by_section: BTreeMap<&str, Vec<(u64, &str)>> = BTreeMap::new();
    for s in &obj.symbols {
        let Some(sec) = s.section.as_deref() else {
            continue;
        };
        if s.name.is_empty() || sections.get(sec).map(|x| x.0) != Some("text") {
            continue;
        }
        by_section.entry(sec).or_default().push((s.value, &s.name));
    }
    let mut out = vec![];
    for (sec, mut syms) in by_section {
        syms.sort();
        let at = l.places.get(sec).copied().unwrap_or(0);
        let end = sections[sec].1;
        for (i, (value, name)) in syms.iter().enumerate() {
            // a second symbol at the same offset (an alias) shares the range
            let next = syms[i + 1..]
                .iter()
                .map(|x| x.0)
                .find(|v| v > value)
                .unwrap_or(end);
            out.push(Entry {
                cpu: Cpu::Cf,
                addr: at + value,
                size: next.saturating_sub(*value),
                name: readable(name),
                layer: Layer::Our,
                module: module.map(String::from),
            });
        }
    }
    // a global symbol and the section's own symbol can name one function twice
    out.sort_by(|a, b| (a.addr, &a.name).cmp(&(b.addr, &b.name)));
    out.dedup_by(|a, b| a.addr == b.addr && a.name == b.name);
    out
}

/// Anchors longer than this name a region (`cf.main` spans the image), not a function or a hook point: they stay out of the map, or they
/// would hide every firmware name under them.
pub const MAX_ANCHOR: u64 = 4096;

/// The code anchors of a port that sit on the ColdFire.
pub fn anchor_entries(port: &Port) -> Vec<Entry> {
    port.anchor
        .iter()
        .filter(|a| a.cpu == "cf" && a.kind == "code" && a.len <= MAX_ANCHOR)
        .map(|a| Entry {
            cpu: Cpu::Cf,
            addr: a.addr,
            size: a.len,
            name: a.name.clone(),
            layer: Layer::Anchor,
            module: None,
        })
        .collect()
}

/// The ship map of an applied plan.
pub fn ship_map(plan: &crate::apply::Plan) -> AddrMap {
    let key = Key {
        device: Some(plan.base.device.clone()),
        firmware: Some(plan.base.version.clone()),
        image_sha256: Some(plan.base.syx_sha256.clone()),
        output_sha256: plan.output.as_ref().map(|o| o.syx_sha256.clone()),
        modules: plan.modules.clone(),
        ..Key::default()
    };
    AddrMap::new("ship", key, plan.addr_entries.clone())
}

/// The map of one packed module: link-relative entries (`obj` linked at base 0 as `pack` does).
pub fn module_map(obj: &Dko, id: &str, dkm_sha256: &str) -> Result<AddrMap> {
    let defined: std::collections::BTreeSet<&str> = obj
        .symbols
        .iter()
        .filter(|s| s.section.is_some())
        .map(|s| s.name.as_str())
        .collect();
    let externs: BTreeMap<String, u64> = obj
        .relocs
        .iter()
        .filter(|r| !defined.contains(r.symbol.as_str()))
        .map(|r| (r.symbol.clone(), 0))
        .collect();
    let linked = crate::link::link(obj, 0, &externs)?;
    let key = Key {
        module: Some(id.to_string()),
        source_sha256: Some(obj.source_sha256.clone()),
        dkm_sha256: Some(dkm_sha256.to_string()),
        ..Key::default()
    };
    Ok(AddrMap::new(
        "module",
        key,
        text_entries(obj, &linked, Some(id)),
    ))
}

const USAGE: &str = "usage: patchkit addr --map FILE... [--loads FILE] [--load ID=BASE]... [--cpu cf|sharc] [--ret] [--format json|text] (ADDR... | --stdin)
  ADDR is hex (0x...) or decimal, optionally CPU:ADDR (sharc:0xb80010). --stdin reads one ADDR per line.
  --loads is a JSON load table {\"modules\":[{\"id\":NAME,\"base\":N}]}; --load adds one module (hex base).
  --ret names a return address (the address minus 2).";

fn number(s: &str) -> Result<u64> {
    let t = s.trim();
    match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        Some(h) => u64::from_str_radix(h, 16),
        None => t.parse(),
    }
    .map_err(|_| Error(format!("bad address {s:?}\n{USAGE}")))
}

/// `patchkit addr`: names for addresses. JSON on stdout: `{"unloaded": [...], "results": [{"cpu", "addr", "name", "text", "layer", "sym_addr",
/// "offset", "size", "module"}]}`; an address no layer holds has `name` null.
pub fn run(rest: &[String]) -> Result<()> {
    let mut maps = vec![];
    let mut loads = LoadTable::default();
    let mut cpu = Cpu::Cf;
    let (mut ret, mut stdin, mut text) = (false, false, false);
    let mut addrs: Vec<String> = vec![];
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        let mut value = |what: &str| -> Result<&String> {
            it.next()
                .ok_or_else(|| Error(format!("{what} needs a value\n{USAGE}")))
        };
        match a.as_str() {
            "--map" => {
                let p = value("--map")?;
                let t = std::fs::read_to_string(p).map_err(|e| Error(format!("{p}: {e}")))?;
                maps.push(AddrMap::parse(&t).map_err(|e| Error(format!("{p}: {e}")))?);
            }
            "--loads" => {
                let p = value("--loads")?;
                let t = std::fs::read_to_string(p).map_err(|e| Error(format!("{p}: {e}")))?;
                let l: LoadTable =
                    serde_json::from_str(&t).map_err(|e| Error(format!("{p}: {e}")))?;
                loads.modules.extend(l.modules);
            }
            "--load" => {
                let v = value("--load")?;
                let (id, base) = v
                    .split_once('=')
                    .ok_or_else(|| Error(format!("--load ID=BASE\n{USAGE}")))?;
                loads.modules.push(dkaddrmap::Load {
                    id: id.into(),
                    base: number(&format!("0x{}", base.trim_start_matches("0x")))?,
                    size: 0,
                });
            }
            "--cpu" => {
                let v = value("--cpu")?;
                cpu = Cpu::parse(v).ok_or_else(|| Error(format!("--cpu cf|sharc\n{USAGE}")))?;
            }
            "--format" => text = value("--format")? == "text",
            "--ret" => ret = true,
            "--stdin" => stdin = true,
            _ => addrs.push(a.clone()),
        }
    }
    if stdin {
        let mut b = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut b)
            .map_err(|e| Error(format!("stdin: {e}")))?;
        addrs.extend(b.lines().filter(|l| !l.trim().is_empty()).map(String::from));
    }
    if maps.is_empty() || addrs.is_empty() {
        return Err(Error(USAGE.into()));
    }
    let r = Resolver::new(&maps, &loads).map_err(Error)?;
    let mut results = vec![];
    for a in &addrs {
        let (c, v) = match a.split_once(':') {
            Some((c, v)) => (
                Cpu::parse(c).ok_or_else(|| Error(format!("bad CPU in {a:?}\n{USAGE}")))?,
                v,
            ),
            None => (cpu, a.as_str()),
        };
        let addr = number(v)?;
        let hit = if ret {
            r.lookup_return(c, addr)
        } else {
            r.lookup(c, addr)
        };
        if text {
            println!(
                "{addr:#x} {}",
                hit.as_ref().map_or("?".to_string(), |h| h.text())
            );
        }
        results.push(serde_json::json!({
            "cpu": c, "addr": addr,
            "name": hit.as_ref().map(|h| &h.name),
            "text": hit.as_ref().map(|h| h.text()),
            "layer": hit.as_ref().map(|h| h.layer),
            "sym_addr": hit.as_ref().map(|h| h.sym_addr),
            "offset": hit.as_ref().map(|h| h.offset),
            "size": hit.as_ref().map(|h| h.size),
            "module": hit.as_ref().and_then(|h| h.module.clone()),
        }));
    }
    if !text {
        println!(
            "{}",
            serde_json::json!({"unloaded": r.unloaded, "results": results})
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dko::{DkoSection, DkoSymbol};

    fn section(name: &str, kind: &str, size: u32) -> DkoSection {
        DkoSection {
            name: name.into(),
            kind: kind.into(),
            align: 4,
            size,
            hex: if kind == "bss" {
                String::new()
            } else {
                "00".repeat(size as usize)
            },
        }
    }

    fn symbol(name: &str, section: &str, value: u64) -> DkoSymbol {
        DkoSymbol {
            name: name.into(),
            section: Some(section.into()),
            value,
            global: true,
        }
    }

    /// One function per section (`-ffunction-sections`), a second function sharing a section, a data symbol and an import.
    fn object() -> Dko {
        Dko {
            format: crate::dko::FORMAT,
            arch: "cf".into(),
            source_sha256: "ab".repeat(32),
            toolchain: Default::default(),
            sections: vec![
                section(".text.first", "text", 16),
                section(".text.pair", "text", 32),
                section(".data.table", "data", 8),
            ],
            symbols: vec![
                symbol("first", ".text.first", 0),
                symbol("first", ".text.first", 0),
                symbol("pair_a", ".text.pair", 0),
                symbol("pair_b", ".text.pair", 12),
                symbol("table", ".data.table", 0),
                symbol("_RNvNtCsdWzhuXZnywa_4dkrt4card4gate", ".text.first", 8),
                DkoSymbol {
                    name: "dk_import".into(),
                    section: None,
                    value: 0,
                    global: true,
                },
            ],
            relocs: vec![],
        }
    }

    #[test]
    fn code_symbols_get_sizes_and_readable_names() {
        let o = object();
        let l = crate::link::link(&o, 0x2000_0000, &BTreeMap::new()).unwrap();
        let e = text_entries(&o, &l, None);
        let at = |n: &str| e.iter().find(|x| x.name == n).map(|x| (x.addr, x.size));
        let first = l.places[".text.first"];
        let pair = l.places[".text.pair"];
        assert_eq!(at("first"), Some((first, 8)));
        assert_eq!(
            at("dkrt::card::gate"),
            Some((first + 8, 8)),
            "a Rust name is demangled, without its hash"
        );
        assert_eq!(at("pair_a"), Some((pair, 12)));
        assert_eq!(at("pair_b"), Some((pair + 12, 20)));
        assert_eq!(e.len(), 4, "no data symbol and no import");
        assert_eq!(readable(".sec..text.put_word"), "put_word");
        assert!(first >= 0x2000_0000);
    }

    #[test]
    fn a_module_map_is_link_relative_and_resolves_after_rebasing() {
        let o = object();
        let map = module_map(&o, "test/demo", "cd").unwrap();
        assert_eq!(map.key.module.as_deref(), Some("test/demo"));
        assert!(
            map.entries
                .iter()
                .all(|e| e.module.as_deref() == Some("test/demo"))
        );
        let loads = LoadTable {
            modules: vec![dkaddrmap::Load {
                id: "test/demo".into(),
                base: 0x2100_0000,
                size: 0x100,
            }],
        };
        let r = Resolver::new(&[map.clone()], &loads).unwrap();
        let pair_b = map.entries.iter().find(|e| e.name == "pair_b").unwrap();
        let hit = r.lookup(Cpu::Cf, 0x2100_0000 + pair_b.addr + 4).unwrap();
        assert_eq!((hit.name.as_str(), hit.offset), ("pair_b", 4));
    }
}
