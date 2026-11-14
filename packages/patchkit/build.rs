//! Embeds the device files (`devices/*.toml`) so that the device list is data: a new device file is one more entry here after a
//! rebuild, with no change to the Rust sources. The file list is written to `$OUT_DIR/device_files.rs` (see `src/devdata.rs`).
//! The `[[firmware]]` tables are left out: patchkit does not read them, and the browser build must not carry stock-image hashes
//! (they come with the port at run time; `apps/web/tests/patchkit.wasm.test.ts`).

use std::fmt::Write;
use std::path::PathBuf;

fn main() {
    src_id();
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../devices");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(".toml"))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    let mut s = String::from("pub const DEVICE_FILES: &[(&str, &str)] = &[\n");
    for n in &names {
        let p = dir.join(n);
        println!("cargo:rerun-if-changed={}", p.display());
        let text = std::fs::read_to_string(&p).unwrap();
        let kept = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join(format!("device-{n}"));
        std::fs::write(&kept, without_firmware(&text)).unwrap();
        let _ = writeln!(
            s,
            "    ({n:?}, include_str!({:?})),",
            kept.to_string_lossy()
        );
    }
    s.push_str("];\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("device_files.rs");
    std::fs::write(out, s).unwrap();
}

/// `text` without its `[[firmware]]` and `[firmware.*]` tables (from such a header to the next header of another table).
fn without_firmware(text: &str) -> String {
    let mut out = String::new();
    let mut skip = false;
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with('[') {
            let name = t.trim_start_matches('[').trim_start();
            skip = name.starts_with("firmware]") || name.starts_with("firmware.");
        }
        if !skip {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// `PATCHKIT_SRC_ID`: a hash of the sources patchkit is built from (FNV-1a over the files in name order): its own `src` and the files of every
/// workspace crate it links through `path` dependencies, transitively (read from the Cargo.toml files, not listed here). A cache of patched images keys on
/// it (`patchkit::SRC_ID`), so a change to the linker, the apply code or a crate that parses or sizes what it packs never serves an image made before it.
fn src_id() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let mut roots = vec![manifest.join("src")];
    let mut seen = vec![manifest.clone()];
    let mut todo = vec![manifest];
    while let Some(dir) = todo.pop() {
        for dep in path_dependencies(&dir) {
            if !seen.contains(&dep) {
                seen.push(dep.clone());
                roots.push(dep.clone());
                todo.push(dep);
            }
        }
    }
    let mut files = vec![];
    let mut dirs = roots;
    while let Some(d) = dirs.pop() {
        println!("cargo:rerun-if-changed={}", d.display());
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            let name = e.file_name();
            if p.is_dir() {
                if !matches!(
                    name.to_str(),
                    Some("target" | "tests" | "examples" | "benches")
                ) {
                    dirs.push(p);
                }
            } else {
                println!("cargo:rerun-if-changed={}", p.display());
                files.push(p);
            }
        }
    }
    files.sort();
    let mut h: u64 = 0xcbf29ce484222325;
    for f in files {
        for b in std::fs::read(&f).unwrap_or_default() {
            h = (h ^ u64::from(b)).wrapping_mul(0x100000001b3);
        }
    }
    println!("cargo:rustc-env=PATCHKIT_SRC_ID={h:016x}");
}

/// The `path = "..."` dependencies (normal, build and target tables, not dev-dependencies) of the crate in `dir`, as canonical directories.
fn path_dependencies(dir: &std::path::Path) -> Vec<PathBuf> {
    let toml = dir.join("Cargo.toml");
    println!("cargo:rerun-if-changed={}", toml.display());
    let text = std::fs::read_to_string(&toml).unwrap_or_default();
    let mut live = false;
    let mut out = vec![];
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            live = t.contains("dependencies") && !t.contains("dev-dependencies");
        } else if live && let Some(at) = t.find("path = \"") {
            let rest = &t[at + 8..];
            if let Some(end) = rest.find('"')
                && let Ok(p) = dir.join(&rest[..end]).canonicalize()
            {
                out.push(p);
            }
        }
    }
    out
}
