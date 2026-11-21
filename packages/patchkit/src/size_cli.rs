//! `patchkit size`: the size gate of the runtime work (docs/PATCHING.md "Size gate").
//!
//! Per device: the stored container of a patch set and of `core/runtime` with its `requires`, against the port's `container_max`
//! (measured without the flash cap, so an image over the limit still gets a number), and the saving of dropping each module of
//! the set. Then the runtime object (`runtime/core/build/runtime.dko.json`): text, data and bss, and text+data by crate and
//! first module path of the Rust v0 symbol names. `--write-baseline FILE` saves every number, `--baseline FILE` prints the deltas.
//!
//! `measure` fills a [`SizeReport`] (numbers and errors, no printing); `render_text` and `to_json` show it.

use crate::apply::{self, Options};
use crate::dko::Dko;
use crate::error::*;
use crate::module::{LibrarySource, library_with};
use crate::port::Registry;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: patchkit size [--set NAME] [--device SHORT,...] [--firmware FILE]... [--no-drop-one]
                     [--runtime-object FILE] [--top N] [--json] [--baseline FILE] [--write-baseline FILE]
  --device filters by the device short name (dt2, dn2, st), also for --firmware files; --help prints this text";

fn num(n: i64) -> String {
    let s = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

fn version_key(v: &str) -> Vec<(u64, bool)> {
    v.split('.')
        .map(|p| {
            let d: String = p.chars().take_while(|c| c.is_ascii_digit()).collect();
            (d.parse().unwrap_or(0), p.len() == d.len())
        })
        .collect()
}

/// Group key of one section name of the runtime object: crate and first module of a Rust symbol, the symbol of a C export.
pub fn group_of(section: &str) -> String {
    let n = ["text", "data", "rodata", "bss"]
        .iter()
        .find_map(|k| {
            section
                .strip_prefix('.')
                .and_then(|s| s.strip_prefix(k))
                .and_then(|s| s.strip_prefix('.'))
        })
        .unwrap_or(section);
    if n.starts_with("_R") {
        let x = format!("{:#}", rustc_demangle::demangle(n));
        let x = match impl_path(&x) {
            Some(p) if p.starts_with('(') => return p,
            Some(p) => format!("{p}::x"),
            None => strip_generics(&x),
        };
        let segs: Vec<&str> = x.split("::").filter(|s| !s.is_empty()).collect();
        match segs.len() {
            0 => "(other) rust".into(),
            1 => segs[0].to_string(),
            _ => segs[..2].join("::"),
        }
    } else if n.starts_with("global.") || n.starts_with("anon.") || n.starts_with(".L") {
        "(consts/strings)".into()
    } else if n.starts_with("dk") || n.starts_with("DK") {
        format!("(export) {n}")
    } else {
        format!("(other) {}", n.chars().take(30).collect::<String>())
    }
}

/// The type path of an impl symbol (`<T as Trait>::m`, `<impl T>::m`, `<&T as X>::m`): what the method belongs to. A builtin type
/// (`<[T]>::m`, `<(A, B)>::m`) is one group. `None` when the name does not start with `<`.
fn impl_path(x: &str) -> Option<String> {
    let inner = x.strip_prefix('<')?;
    // the matching `>` and the first ` as ` at depth 0
    let (mut depth, mut end, mut as_at) = (0usize, inner.len(), None);
    for (i, c) in inner.char_indices() {
        match c {
            '<' => depth += 1,
            '>' if depth == 0 => {
                end = i;
                break;
            }
            '>' => depth -= 1,
            ' ' if depth == 0 && as_at.is_none() && inner[i..].starts_with(" as ") => {
                as_at = Some(i)
            }
            _ => {}
        }
    }
    let ty = &inner[..as_at.unwrap_or(end).min(end)];
    let mut ty = ty.trim_start_matches(['&', '*']);
    for p in ["mut ", "const ", "dyn ", "impl "] {
        ty = ty.strip_prefix(p).unwrap_or(ty);
    }
    if !ty.starts_with(|c: char| c.is_alphabetic() || c == '_') {
        return Some("(builtin impl)".into());
    }
    let len = ty
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'))
        .unwrap_or(ty.len());
    let ty = &ty[..len];
    // a nominal type is a full path; one segment is a primitive or a generic parameter (a blanket impl)
    if !ty.contains("::") {
        return Some("(generic impl)".into());
    }
    Some(ty.to_string())
}

/// `s` without its `<...>` groups (nested too); a `>` without an opening `<` stays.
fn strip_generics(s: &str) -> String {
    let (mut depth, mut out) = (0usize, String::new());
    for c in s.chars() {
        match c {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

// ---- arguments

const VALUE_FLAGS: &[&str] = &[
    "--set",
    "--device",
    "--firmware",
    "--top",
    "--runtime-object",
    "--baseline",
    "--write-baseline",
];
const BOOL_FLAGS: &[&str] = &["--json", "--no-drop-one", "--help", "-h"];

/// The arguments after `size`: unknown flags and flags without a value are errors (main.rs `flag` does neither check).
#[derive(Default)]
struct Cli {
    values: Vec<(String, String)>,
    bools: Vec<String>,
}

impl Cli {
    fn parse(rest: &[String]) -> Result<Cli> {
        let mut c = Cli::default();
        let mut it = rest.iter();
        while let Some(a) = it.next() {
            if BOOL_FLAGS.contains(&a.as_str()) {
                c.bools.push(a.clone());
            } else if VALUE_FLAGS.contains(&a.as_str()) {
                match it.next() {
                    Some(v) if !v.starts_with("--") => c.values.push((a.clone(), v.clone())),
                    _ => return Err(err!("size: {a} needs a value\n{USAGE}")),
                }
            } else {
                return Err(err!("size: unknown argument {a}\n{USAGE}"));
            }
        }
        Ok(c)
    }
    fn has(&self, name: &str) -> bool {
        self.bools.iter().any(|b| b == name)
    }
    fn one(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .rev()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
    fn list(&self, name: &str) -> Vec<String> {
        self.values
            .iter()
            .filter(|(k, _)| k == name)
            .flat_map(|(_, v)| v.split(',').map(String::from))
            .filter(|s| !s.is_empty())
            .collect()
    }
}

// ---- the report

#[derive(Default)]
struct DropOne {
    module: String,
    /// bytes the image shrinks when the module leaves the set; `None` when that build failed (see the device's errors)
    saving: Option<i64>,
    readded: bool,
}

#[derive(Default)]
struct DeviceReport {
    short: String,
    version: String,
    file: String,
    set: String,
    limit: i64,
    stored_set: Option<i64>,
    stored_runtime: Option<i64>,
    runtime_closure: Vec<String>,
    drops: Vec<DropOne>,
    errors: Vec<String>,
}

#[derive(Default)]
struct RuntimeReport {
    path: String,
    text: i64,
    data: i64,
    bss: i64,
    groups: BTreeMap<String, i64>,
}

#[derive(Default)]
struct SizeReport {
    devices: Vec<DeviceReport>,
    runtime: Option<RuntimeReport>,
    /// failures that belong to no device: an unreadable firmware file, a missing runtime object
    errors: Vec<String>,
    top: usize,
}

impl SizeReport {
    /// every number by a path-like key (the baseline file)
    fn flat(&self) -> BTreeMap<String, i64> {
        let mut m = BTreeMap::new();
        for d in &self.devices {
            let s = &d.short;
            if let Some(n) = d.stored_set {
                m.insert(format!("{s}/stored/set"), n);
            }
            if let Some(n) = d.stored_runtime {
                m.insert(format!("{s}/stored/runtime"), n);
            }
            for x in &d.drops {
                if let Some(n) = x.saving {
                    m.insert(format!("{s}/drop/{}", x.module), n);
                }
            }
        }
        if let Some(r) = &self.runtime {
            m.insert("runtime/text".into(), r.text);
            m.insert("runtime/data".into(), r.data);
            m.insert("runtime/bss".into(), r.bss);
            for (k, v) in &r.groups {
                m.insert(format!("runtime/group/{k}"), *v);
            }
        }
        m
    }

    /// the key prefixes this run measured, for the baseline compare
    fn scope(&self) -> Vec<String> {
        let mut p: Vec<String> = self
            .devices
            .iter()
            .map(|d| format!("{}/", d.short))
            .collect();
        if self.runtime.is_some() {
            p.push("runtime/".into());
        }
        p
    }

    fn error_count(&self) -> usize {
        self.errors.len() + self.devices.iter().map(|d| d.errors.len()).sum::<usize>()
    }
}

fn opt(n: Option<i64>) -> serde_json::Value {
    n.map_or(serde_json::Value::Null, Into::into)
}

fn to_json(r: &SizeReport) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    for d in &r.devices {
        let drops: serde_json::Map<String, serde_json::Value> = d
            .drops
            .iter()
            .map(|x| (x.module.clone(), opt(x.saving)))
            .collect();
        m.insert(
            d.short.clone(),
            serde_json::json!({
                "version": d.version,
                "container_max": d.limit,
                "stored_set": opt(d.stored_set),
                "stored_runtime": opt(d.stored_runtime),
                "drop_one": drops,
                "errors": d.errors,
            }),
        );
    }
    if let Some(o) = &r.runtime {
        m.insert(
            "runtime_object".into(),
            serde_json::json!({"text": o.text, "data": o.data, "bss": o.bss, "groups": o.groups}),
        );
    }
    m.insert("errors".into(), r.errors.clone().into());
    m.into()
}

fn render_text(r: &SizeReport) -> String {
    use std::fmt::Write;
    let mut o = String::new();
    for d in &r.devices {
        let _ = writeln!(
            o,
            "== {} {} ({}), set {}",
            d.short, d.version, d.file, d.set
        );
        if let Some(n) = d.stored_set {
            let room = d.limit - n;
            let _ = writeln!(
                o,
                "  stored container    {:>10}  limit {}  {} {}",
                num(n),
                num(d.limit),
                if room >= 0 { "headroom +" } else { "OVER by " },
                num(room.abs())
            );
        }
        if let Some(n) = d.stored_runtime {
            let _ = writeln!(
                o,
                "  runtime + requires  {:>10}  ({})",
                num(n),
                d.runtime_closure.join(", ")
            );
        }
        if !d.drops.is_empty() {
            let _ = writeln!(
                o,
                "  drop-one saving (bytes the image shrinks when the module leaves the set):"
            );
            for x in &d.drops {
                let Some(n) = x.saving else { continue };
                let note = if x.readded {
                    "  (re-added by requires or a device rule)"
                } else {
                    ""
                };
                let _ = writeln!(o, "    {:<24}{:>9}{note}", x.module, num(n));
            }
        }
        for e in &d.errors {
            let _ = writeln!(o, "  error: {e}");
        }
    }
    if let Some(g) = &r.runtime {
        let total = g.text + g.data;
        let _ = writeln!(o, "== runtime object {}", g.path);
        let _ = writeln!(
            o,
            "  text {}  data+rodata {}  bss {}  (image carries {} non-bss bytes)",
            num(g.text),
            num(g.data),
            num(g.bss),
            num(total)
        );
        let mut by: Vec<(&String, &i64)> = g.groups.iter().collect();
        by.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        let _ = writeln!(o, "  text+data by crate and first module (top {}):", r.top);
        for (k, v) in by.iter().take(r.top) {
            let pct = if total > 0 {
                100.0 * **v as f64 / total as f64
            } else {
                0.0
            };
            let _ = writeln!(o, "    {:>8} {pct:>5.1}%  {k}", num(**v));
        }
    }
    for e in &r.errors {
        let _ = writeln!(o, "error: {e}");
    }
    o
}

// ---- measuring

fn newest_stock_firmware(root: &Path, reg: &Registry, want: &[String]) -> Result<Vec<PathBuf>> {
    // the newest stock file of each device that has a port and a file in firmware/ (the sha256 decides, not the name)
    let dir = root.join("firmware");
    let files: Vec<(String, PathBuf)> = std::fs::read_dir(&dir)
        .map_err(|e| err!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "syx"))
        .filter_map(|p| Some((sha256_hex(&std::fs::read(&p).ok()?), p)))
        .collect();
    let mut best: BTreeMap<String, (Vec<(u64, bool)>, PathBuf)> = BTreeMap::new();
    for p in &reg.ports {
        let Some((_, path)) = files.iter().find(|(h, _)| *h == p.port.syx_sha256) else {
            continue;
        };
        let k = version_key(&p.port.version);
        let cur = best
            .entry(p.port.device.clone())
            .or_insert((k.clone(), path.clone()));
        if k > cur.0 {
            *cur = (k, path.clone());
        }
    }
    let fws: Vec<PathBuf> = best
        .into_iter()
        .filter(|(dev, _)| want.is_empty() || want.contains(dev))
        .map(|(_, (_, path))| path)
        .collect();
    if fws.is_empty() {
        return Err(err!(
            "size: no stock firmware found in {} (or pass --firmware FILE)",
            dir.display()
        ));
    }
    Ok(fws)
}

/// Measure one firmware file. A failure before the device is known goes to `rep.errors`; after that to the device's own errors.
fn measure_device(
    root: &Path,
    reg: &Registry,
    fw_path: &Path,
    set: &str,
    drop_one: bool,
    want: &[String],
    rep: &mut SizeReport,
) {
    let setup = || -> Result<_> {
        let raw = std::fs::read(fw_path).map_err(|e| err!("{}: {e}", fw_path.display()))?;
        let fw = crate::firmware::Firmware::parse(&raw)?;
        let short = fw.device.short().to_string();
        let port = reg
            .find(&fw.syx_sha256)
            .ok_or_else(|| err!("no port for {}", fw_path.display()))?;
        let info =
            crate::devdata::by_short(&short).ok_or_else(|| err!("no device file for {short}"))?;
        let roots: Vec<String> = info.patch_set(set)?.to_vec();
        Ok((raw, short, port, info, roots))
    };
    let (raw, short, port, info, roots) = match setup() {
        Ok(x) => x,
        Err(e) => {
            rep.errors.push(format!("{}: {e}", fw_path.display()));
            return;
        }
    };
    if !want.is_empty() && !want.contains(&short) {
        return;
    }
    let mut d = DeviceReport {
        short: short.clone(),
        version: port.port.version.clone(),
        file: fw_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        set: set.into(),
        limit: port.limits.container_max as i64,
        ..Default::default()
    };
    let src = LibrarySource::default();
    let lib = match library_with(root, &src, false, Some(&[]))
        .and_then(|m| m.with_requires(&roots))
        .and_then(|(full, _)| library_with(root, &src, false, Some(&full)))
    {
        Ok(l) => l,
        Err(e) => {
            d.errors.push(format!("library: {e}"));
            rep.devices.push(d);
            return;
        }
    };
    // the limit check of `apply` stays: this measures without the flash cap
    let opts = Options {
        record: true,
        flash_cap: false,
    };
    // the requires closure, plus the modules a device rule (`needed_when`) makes mandatory beside a module of it
    // (`apply` only checks that rule, it does not add the modules)
    let close = |ids: &[String]| -> Result<Vec<String>> {
        let mut all = lib.with_requires(ids)?.0;
        loop {
            let more: Vec<String> = lib
                .modules
                .iter()
                .filter(|m| !all.iter().any(|a| a == m.id()))
                .filter(|m| {
                    m.manifest.module.needed_when.iter().any(|nw| {
                        info.switches.get(&nw.switch).copied().unwrap_or(false)
                            && all.contains(&nw.with)
                    })
                })
                .map(|m| m.id().to_string())
                .collect();
            if more.is_empty() {
                return Ok(all);
            }
            all = lib.with_requires(&[all, more].concat())?.0;
        }
    };
    let measure = |ids: &[String]| -> Result<i64> {
        let all = close(ids)?;
        let a = apply::apply_with(&raw, reg, &lib, &all, opts)?;
        Ok(a.plan.output.as_ref().unwrap().container_bytes as i64)
    };
    let keep = |what: &str, r: Result<i64>, d: &mut DeviceReport| match r {
        Ok(n) => Some(n),
        Err(e) => {
            d.errors.push(format!("{what}: {e}"));
            None
        }
    };
    d.stored_set = keep("stored/set", measure(&roots), &mut d);
    let rt = ["core/runtime".to_string()];
    match close(&rt) {
        Ok(c) => {
            d.runtime_closure = c;
            d.stored_runtime = keep("stored/runtime", measure(&rt), &mut d);
        }
        Err(e) => d.errors.push(format!("stored/runtime: {e}")),
    }
    if drop_one && let Some(full_n) = d.stored_set {
        for m in &roots {
            let rest: Vec<String> = roots.iter().filter(|r| *r != m).cloned().collect();
            let readded = close(&rest).is_ok_and(|c| c.contains(m));
            let saving = keep(&format!("drop {m}"), measure(&rest), &mut d).map(|n| full_n - n);
            d.drops.push(DropOne {
                module: m.clone(),
                saving,
                readded,
            });
        }
    }
    rep.devices.push(d);
}

fn measure_runtime_object(path: &Path) -> Result<RuntimeReport> {
    let text = std::fs::read_to_string(path).map_err(|e| err!("{}: {e}", path.display()))?;
    let dko = Dko::parse(&text).map_err(|e| err!("{}: {e}", path.display()))?;
    let mut r = RuntimeReport {
        path: path.display().to_string(),
        ..Default::default()
    };
    for s in &dko.sections {
        let n = s.size as i64;
        match s.kind.as_str() {
            "text" => r.text += n,
            "data" => r.data += n,
            _ => r.bss += n,
        }
        if s.kind != "bss" {
            *r.groups.entry(group_of(&s.name)).or_default() += n;
        }
    }
    Ok(r)
}

// ---- baseline

#[derive(Default)]
struct Delta {
    changed: Vec<(String, i64, i64)>,
    new: Vec<(String, i64)>,
    gone: Vec<(String, i64)>,
    same: usize,
}

/// Compare the numbers of this run with a baseline; only the baseline keys under `scope` (the measured devices and the runtime object) count,
/// and the drop-one keys only when this run measured them (`drop_one`).
fn compare(
    base: &BTreeMap<String, i64>,
    now: &BTreeMap<String, i64>,
    scope: &[String],
    drop_one: bool,
) -> Delta {
    let mut d = Delta::default();
    for (k, v) in now {
        match base.get(k) {
            Some(b) if b == v => d.same += 1,
            Some(b) => d.changed.push((k.clone(), *b, *v)),
            None => d.new.push((k.clone(), *v)),
        }
    }
    for (k, b) in base {
        let measured =
            scope.iter().any(|p| k.starts_with(p.as_str())) && (drop_one || !k.contains("/drop/"));
        if !now.contains_key(k) && measured {
            d.gone.push((k.clone(), *b));
        }
    }
    d
}

fn delta_text(d: &Delta) -> String {
    use std::fmt::Write;
    let mut o = String::from("== deltas against the baseline (now - baseline)\n");
    for (k, b, v) in &d.changed {
        let _ = writeln!(
            o,
            "  {k}: {} -> {}  ({}{})",
            num(*b),
            num(*v),
            if v > b { "+" } else { "" },
            num(v - b)
        );
    }
    for (k, v) in &d.new {
        let _ = writeln!(o, "  {k}: {}  (new)", num(*v));
    }
    for (k, b) in &d.gone {
        let _ = writeln!(o, "  {k}: {}  (gone)", num(*b));
    }
    let _ = writeln!(o, "  {} numbers unchanged", d.same);
    o
}

fn delta_json(d: &Delta) -> serde_json::Value {
    serde_json::json!({
        "changed": d.changed.iter().map(|(k, b, v)| (k.clone(), serde_json::json!({"baseline": b, "now": v, "delta": v - b}))).collect::<serde_json::Map<_, _>>(),
        "new": d.new.iter().cloned().collect::<BTreeMap<_, _>>(),
        "gone": d.gone.iter().cloned().collect::<BTreeMap<_, _>>(),
        "unchanged": d.same,
    })
}

// ---- command

pub fn run(root: &Path, rest: &[String]) -> Result<()> {
    let cli = Cli::parse(rest)?;
    if cli.has("--help") || cli.has("-h") {
        println!("{USAGE}");
        return Ok(());
    }
    let reg = Registry::from_dir(&root.join("ports"))?;
    let set = cli.one("--set").unwrap_or("device").to_string();
    let want = cli.list("--device");
    if let Some(bad) = want.iter().find(|w| crate::devdata::by_short(w).is_none()) {
        return Err(err!(
            "size: unknown device {bad} (known: {})",
            crate::devdata::short_list()
        ));
    }
    let top = cli
        .one("--top")
        .map_or(Ok(30), |s| s.parse().map_err(|_| err!("size: --top N")))?;
    let drop_one = !cli.has("--no-drop-one");
    let json = cli.has("--json");
    // the newest-firmware search filters by the port's device field, which is the short name; `--firmware` files are filtered once parsed
    let mut fws: Vec<PathBuf> = cli
        .list("--firmware")
        .into_iter()
        .map(PathBuf::from)
        .collect();
    if fws.is_empty() {
        fws = newest_stock_firmware(root, &reg, &want)?;
    }
    let mut rep = SizeReport {
        top,
        ..Default::default()
    };
    for fw in &fws {
        measure_device(root, &reg, fw, &set, drop_one, &want, &mut rep);
    }
    if rep.devices.is_empty() && rep.errors.is_empty() {
        return Err(err!(
            "size: --device {} matches none of the firmware files",
            want.join(",")
        ));
    }
    let obj = cli
        .one("--runtime-object")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("runtime/core/build/runtime.dko.json"));
    match measure_runtime_object(&obj) {
        Ok(r) => rep.runtime = Some(r),
        Err(e) => rep.errors.push(format!("runtime object: {e}")),
    }
    let now = rep.flat();
    let delta = match cli.one("--baseline") {
        Some(p) => {
            let t = std::fs::read_to_string(p).map_err(|e| err!("{p}: {e}"))?;
            let base: BTreeMap<String, i64> =
                serde_json::from_str(&t).map_err(|e| err!("{p}: {e}"))?;
            Some(compare(&base, &now, &rep.scope(), drop_one))
        }
        None => None,
    };
    let n_err = rep.error_count();
    // a baseline of a run with errors would miss numbers, so it is not written
    let wrote = match cli.one("--write-baseline") {
        Some(p) if n_err == 0 => {
            let s = serde_json::to_string_pretty(&now)? + "\n";
            std::fs::write(p, s).map_err(|e| err!("{p}: {e}"))?;
            Some(p)
        }
        _ => None,
    };
    if json {
        let mut v = to_json(&rep);
        if let (Some(d), Some(m)) = (&delta, v.as_object_mut()) {
            m.insert("delta_vs_baseline".into(), delta_json(d));
        }
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else {
        print!("{}", render_text(&rep));
        if let Some(d) = &delta {
            print!("{}", delta_text(d));
        }
        if let Some(p) = wrote {
            println!("wrote baseline {p} ({} numbers)", now.len());
        }
    }
    if n_err > 0 {
        return Err(err!(
            "size: {n_err} error(s) above{}",
            if cli.one("--write-baseline").is_some() {
                "; baseline not written"
            } else {
                ""
            }
        ));
    }
    Ok(())
}
