//! `patchkit objdiff OLD NEW`: do two objects of the same code differ only in names and placement?
//!
//! Proof for moves of Rust items between modules. Mangled names contain the module path, the function order follows it, and
//! relocations point at section symbols, so a byte compare of the objects fails for a perfect move. Here:
//! - functions and named data are matched by a normalized name (demangled, module path removed; a C symbol keeps its exact name);
//! - an anonymous constant (`.rodata.global.*`) is matched by its content;
//! - a relocation is compared by its normalized target plus addend, never by section or address;
//! - code is compared byte by byte with the relocated fields masked, and the first difference is shown as an instruction.
//!
//! Items with the same normalized name (two `new` in different modules) are paired by equal content first, then in order of content.

use crate::dko::{Dko, DkoSection};
use crate::error::*;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// One difference. `kind`: `only-old`, `only-new`, `differs`, `c-symbol` (an exact-name symbol was added, removed or changed) or `extern`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Difference {
    pub kind: String,
    pub name: String,
    pub detail: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Report {
    pub identical: bool,
    pub functions: usize,
    pub data_objects: usize,
    pub differences: Vec<Difference>,
}

impl Report {
    pub fn text(&self) -> String {
        if self.identical {
            return format!(
                "identical: {} functions, {} data objects\n",
                self.functions, self.data_objects
            );
        }
        let mut s = format!(
            "differences: {} ({} functions and {} data objects match)\n",
            self.differences.len(),
            self.functions,
            self.data_objects
        );
        for d in &self.differences {
            s.push_str(&format!("{:9} {}\n", d.kind, d.name));
            for l in d.detail.lines() {
                s.push_str(&format!("            {l}\n"));
            }
        }
        s
    }
}

/// A name without crate and module path: `_RNvNtCs..4dkrt3foo4rd32` is `rd32`, `<dkrt::a::T>::m` is `<T>::m`. A name that is not Rust-mangled
/// (a C symbol, `#[no_mangle]`) stays exact.
pub fn norm_name(name: &str) -> String {
    if !name.starts_with("_R") {
        return name.to_string();
    }
    let Ok(d) = rustc_demangle::try_demangle(name) else {
        return name.to_string();
    };
    let s = format!("{d:#}");
    let b = s.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let start = c.is_ascii_alphabetic() || c == b'_';
        let prev_ok = i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_');
        if start && prev_ok {
            let mut j = i;
            while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                j += 1;
            }
            // `module::` before another identifier: a path prefix. Before `<` or `{` it names an item (`f::{closure#0}`): kept.
            if s[j..].starts_with("::")
                && b.get(j + 2)
                    .is_some_and(|n| n.is_ascii_alphabetic() || *n == b'_')
            {
                i = j + 2;
                continue;
            }
            out.push_str(&s[i..j]);
            i = j;
            continue;
        }
        out.push(c as char);
        i += 1;
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Text,
    Named,
    Anon,
}

struct Item {
    /// normalized name (`Anon`: empty)
    name: String,
    /// the section's name in the object (for messages)
    raw: String,
    class: Class,
    align: u32,
    size: u32,
    bss: bool,
    /// bytes with every relocated field zeroed
    bytes: Vec<u8>,
    /// (offset, type, normalized target + addend)
    relocs: Vec<(u64, String, String)>,
}

impl Item {
    fn sig(&self) -> String {
        let mut h = Sha256::new();
        h.update(self.align.to_be_bytes());
        h.update(self.size.to_be_bytes());
        h.update([self.bss as u8]);
        h.update(&self.bytes);
        for (o, t, x) in &self.relocs {
            h.update(o.to_be_bytes());
            h.update(t.as_bytes());
            h.update([0]);
            h.update(x.as_bytes());
            h.update([0]);
        }
        hex(&h.finalize()[..12])
    }
}

const PREFIXES: [&str; 4] = [".text.", ".rodata.", ".data.", ".bss."];

fn split_prefix(name: &str) -> (&str, &str) {
    for p in PREFIXES {
        if let Some(r) = name.strip_prefix(p) {
            return (p, r);
        }
    }
    ("", name)
}

fn is_anon(rest: &str) -> bool {
    ["global.", ".L", "anon.", "str.", "alloc_"]
        .iter()
        .any(|p| rest.starts_with(p))
}

fn class_of(s: &DkoSection) -> Class {
    let (_, rest) = split_prefix(&s.name);
    if s.kind == "text" {
        Class::Text
    } else if is_anon(rest) {
        Class::Anon
    } else {
        Class::Named
    }
}

/// The label of a section as a relocation target.
fn label(s: &DkoSection, content: &HashMap<&str, String>) -> String {
    let (p, rest) = split_prefix(&s.name);
    match class_of(s) {
        Class::Anon => format!("anon:{}", content[s.name.as_str()]),
        _ => format!("{}{}", p, norm_name(rest)),
    }
}

fn load(o: &Dko, pool: bool) -> Result<(Vec<Item>, BTreeSet<String>)> {
    let secs: HashMap<&str, &DkoSection> =
        o.sections.iter().map(|s| (s.name.as_str(), s)).collect();
    // the content of an anonymous section (no relocations: a target label must not depend on other labels)
    let mut content: HashMap<&str, String> = HashMap::new();
    for s in &o.sections {
        let mut h = Sha256::new();
        h.update(s.align.to_be_bytes());
        h.update(s.size.to_be_bytes());
        h.update(unhex(&s.hex)?);
        let shown: String = unhex(&s.hex)?
            .iter()
            .take(20)
            .map(|&b| {
                if (32..127).contains(&b) && b != b'"' {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        content.insert(
            s.name.as_str(),
            format!("{}\"{shown}\"", hex(&h.finalize()[..6])),
        );
    }
    let defined: HashMap<&str, (&str, u64)> = o
        .symbols
        .iter()
        .filter(|y| !y.name.starts_with(".sec."))
        .filter_map(|y| {
            y.section
                .as_deref()
                .map(|s| (y.name.as_str(), (s, y.value)))
        })
        .collect();
    // Pool mode: the compiler merges string literals into blobs whose layout depends on the order of the code, so a relocation into an
    // anonymous constant has no stable blob or offset (a pointer past the end of a literal may even sit in another blob). The label is
    // only "pool": the code bytes, the position of every pool reference and all other relocations still have to match.
    let at_label = |s: &DkoSection, addend: i64| -> String {
        if pool && class_of(s) == Class::Anon {
            return "pool".into();
        }
        format!("{}{:+}", label(s, &content), addend)
    };
    let target = |sym: &str, addend: i64| -> String {
        if let Some(sn) = sym.strip_prefix(".sec.") {
            if let Some(s) = secs.get(sn) {
                return at_label(s, addend);
            }
        }
        if let Some((sn, v)) = defined.get(sym) {
            if let Some(s) = secs.get(sn) {
                return at_label(s, *v as i64 + addend);
            }
        }
        format!("{}{:+}", norm_name(sym), addend)
    };
    let mut items = vec![];
    for s in &o.sections {
        let mut bytes = if s.kind == "bss" {
            vec![]
        } else {
            unhex(&s.hex)?
        };
        let mut relocs = vec![];
        for r in o.relocs.iter().filter(|r| r.section == s.name) {
            let w = match r.rtype.as_str() {
                "cf.pc8" => 1,
                "cf.pc16" => 2,
                _ => 4,
            };
            let at = r.offset as usize;
            if at + w <= bytes.len() {
                bytes[at..at + w].fill(0);
            }
            relocs.push((r.offset, r.rtype.clone(), target(&r.symbol, r.addend)));
        }
        relocs.sort();
        let class = class_of(s);
        let (_, rest) = split_prefix(&s.name);
        items.push(Item {
            name: if class == Class::Anon {
                String::new()
            } else {
                norm_name(rest)
            },
            raw: s.name.clone(),
            class,
            align: s.align,
            size: s.size,
            bss: s.kind == "bss",
            bytes,
            relocs,
        });
    }
    let externs = o
        .symbols
        .iter()
        .filter(|y| y.section.is_none())
        .map(|y| norm_name(&y.name))
        .collect();
    Ok((items, externs))
}

fn describe(i: &Item) -> String {
    if i.class == Class::Anon {
        let shown: String = i
            .bytes
            .iter()
            .take(24)
            .map(|&b| {
                if (32..127).contains(&b) {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        format!("anonymous constant of {} bytes {shown:?}", i.size)
    } else {
        i.name.clone()
    }
}

fn c_visible(i: &Item) -> bool {
    i.class != Class::Anon && !i.raw.contains("._R")
}

/// The first difference of two items with equal normalized names, as text.
fn explain(old: &Item, new: &Item) -> String {
    let mut out = vec![];
    if old.align != new.align {
        out.push(format!("align {} -> {}", old.align, new.align));
    }
    if old.size != new.size {
        out.push(format!("size {} -> {} bytes", old.size, new.size));
    }
    if old.bss {
        return out.join("\n");
    }
    // first differing byte or relocation
    let n = old.bytes.len().min(new.bytes.len());
    let mut at = (0..n).find(|&k| old.bytes[k] != new.bytes[k]);
    let mut why = "bytes";
    for (a, b) in old.relocs.iter().zip(&new.relocs) {
        if a != b {
            if at.is_none_or(|x| (a.0.min(b.0) as usize) < x) {
                at = Some(a.0.min(b.0) as usize);
                why = "relocation";
            }
            break;
        }
    }
    if at.is_none() && old.relocs.len() != new.relocs.len() {
        let l = old.relocs.len().min(new.relocs.len());
        let o = old
            .relocs
            .get(l)
            .or(new.relocs.get(l))
            .map(|r| r.0 as usize);
        at = o;
        why = "relocation count";
    }
    let Some(at) = at else {
        return out.join("\n");
    };
    out.push(format!(
        "first difference at +{at:#x} ({why}); old, then new"
    ));
    if old.class == Class::Text {
        // instruction starts of the old side up to `at`
        let mut starts = vec![];
        let mut p = 0usize;
        while p <= at && p < old.bytes.len() {
            starts.push(p);
            let l = coldfire::decode_at(&old.bytes, 0, p).map_or(2, |i| (i.len as usize).max(2));
            p += l;
        }
        let first = starts.last().copied().unwrap_or(0);
        for &s in starts.iter().rev().take(3).rev() {
            if s != first {
                out.push(format!("  {}", insn_line(old, s)));
            }
        }
        out.push(format!("- {}", insn_line(old, first)));
        out.push(format!("+ {}", insn_line(new, first)));
    } else {
        let w = |i: &Item| {
            let e = (at + 8).min(i.bytes.len());
            hex(i.bytes.get(at..e).unwrap_or(&[]))
        };
        out.push(format!("- {}", w(old)));
        out.push(format!("+ {}", w(new)));
        for r in old.relocs.iter().filter(|r| r.0 as usize == at) {
            out.push(format!("- reloc {} {}", r.1, r.2));
        }
        for r in new.relocs.iter().filter(|r| r.0 as usize == at) {
            out.push(format!("+ reloc {} {}", r.1, r.2));
        }
    }
    out.join("\n")
}

fn insn_line(i: &Item, at: usize) -> String {
    let (len, text) = match coldfire::decode_at(&i.bytes, 0, at) {
        Some(x) => (
            (x.len as usize).max(2),
            format!("{}{}", x.mnemonic(), x.size.suffix()),
        ),
        None => (2, "??".to_string()),
    };
    let end = (at + len).min(i.bytes.len());
    let mut s = format!("+{at:#06x}  {:<12} {}", hex(&i.bytes[at..end]), text);
    for r in i
        .relocs
        .iter()
        .filter(|r| (r.0 as usize) < end && r.0 as usize + 4 > at)
    {
        s.push_str(&format!("  [reloc +{:#x} {}]", r.0 as usize - at, r.2));
    }
    s
}

/// Compare two objects.
pub fn diff(old: &Dko, new: &Dko) -> Result<Report> {
    diff_with(old, new, false)
}

/// Compare two objects; `pool` ignores the layout of merged anonymous constants (see `load`) and does not list them.
pub fn diff_with(old: &Dko, new: &Dko, pool: bool) -> Result<Report> {
    if old.arch != "cf" || new.arch != "cf" {
        return Err(err!(
            "objdiff compares ColdFire objects (got {} and {})",
            old.arch,
            new.arch
        ));
    }
    let (a, ae) = load(old, pool)?;
    let (b, be) = load(new, pool)?;
    let mut rep = Report::default();
    let mut push = |kind: &str, name: String, detail: String| {
        rep.differences.push(Difference {
            kind: kind.into(),
            name,
            detail,
        })
    };
    // group by (class, name)
    let key = |i: &Item| (i.class as u8, i.name.clone());
    let mut ga: BTreeMap<(u8, String), Vec<&Item>> = BTreeMap::new();
    let mut gb: BTreeMap<(u8, String), Vec<&Item>> = BTreeMap::new();
    a.iter().for_each(|i| ga.entry(key(i)).or_default().push(i));
    b.iter().for_each(|i| gb.entry(key(i)).or_default().push(i));
    let (mut funcs, mut datas) = (0usize, 0usize);
    let keys: BTreeSet<_> = ga.keys().chain(gb.keys()).cloned().collect();
    let flag = |kind_only: &str, i: &Item, push: &mut dyn FnMut(&str, String, String)| {
        let kind = if c_visible(i) { "c-symbol" } else { kind_only };
        let detail = if c_visible(i) {
            format!("exact-name symbol {} ({kind_only})", i.raw)
        } else {
            String::new()
        };
        push(kind, describe(i), detail);
    };
    for k in keys {
        let mut xs = ga.remove(&k).unwrap_or_default();
        let mut ys = gb.remove(&k).unwrap_or_default();
        let count = |n: usize, c: u8, f: &mut usize, d: &mut usize| {
            if c == Class::Text as u8 {
                *f += n
            } else {
                *d += n
            }
        };
        // equal content first
        let mut i = 0;
        while i < xs.len() {
            let s = xs[i].sig();
            if let Some(j) = ys.iter().position(|y| y.sig() == s) {
                ys.remove(j);
                xs.remove(i);
                count(1, k.0, &mut funcs, &mut datas);
            } else {
                i += 1;
            }
        }
        if k.0 == Class::Anon as u8 {
            if pool {
                continue;
            }
            xs.iter().for_each(|x| flag("only-old", x, &mut push));
            ys.iter().for_each(|y| flag("only-new", y, &mut push));
            continue;
        }
        xs.sort_by_key(|x| x.sig());
        ys.sort_by_key(|y| y.sig());
        let n = xs.len().min(ys.len());
        for j in 0..n {
            if c_visible(xs[j]) && xs[j].raw != ys[j].raw {
                push(
                    "c-symbol",
                    describe(xs[j]),
                    format!("renamed {} -> {}", xs[j].raw, ys[j].raw),
                );
            }
            push("differs", describe(xs[j]), explain(xs[j], ys[j]));
        }
        xs[n..].iter().for_each(|x| flag("only-old", x, &mut push));
        ys[n..].iter().for_each(|y| flag("only-new", y, &mut push));
    }
    for e in ae.difference(&be) {
        push("extern", e.clone(), "undefined symbol only in old".into());
    }
    for e in be.difference(&ae) {
        push("extern", e.clone(), "undefined symbol only in new".into());
    }
    rep.functions = funcs;
    rep.data_objects = datas;
    rep.identical = rep.differences.is_empty();
    Ok(rep)
}

/// `patchkit objdiff OLD NEW [--format text|json]`; returns the exit code.
pub fn run(rest: &[String]) -> Result<i32> {
    const U: &str =
        "usage: patchkit objdiff OLD.dko.json NEW.dko.json [--format text|json] [--pool]";
    let mut pool = false;
    let mut files = vec![];
    let mut format = "text".to_string();
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        if a == "--pool" {
            pool = true;
        } else if a == "--format" {
            format = it.next().ok_or_else(|| Error(U.into()))?.clone();
        } else if a.starts_with("--") {
            return Err(Error(U.into()));
        } else {
            files.push(a.clone());
        }
    }
    if files.len() != 2 || !matches!(format.as_str(), "text" | "json") {
        return Err(Error(U.into()));
    }
    let read = |f: &str| -> Result<Dko> {
        Ok(Dko::parse(
            &std::fs::read_to_string(f).map_err(|e| Error(format!("{f}: {e}")))?,
        )?)
    };
    let rep = diff_with(&read(&files[0])?, &read(&files[1])?, pool)?;
    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&rep)?);
    } else {
        print!("{}", rep.text());
    }
    Ok(if rep.identical { 0 } else { 1 })
}
