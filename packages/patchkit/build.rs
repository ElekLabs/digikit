//! Embeds the device files (`devices/*.toml`) so that the device list is data: a new device file is one more entry here after a
//! rebuild, with no change to the Rust sources. The file list is written to `$OUT_DIR/device_files.rs` (see `src/devdata.rs`).
//! The files are embedded whole, `[[firmware]]` tables included: the web app ships device files with their stock-image hashes to
//! identify the firmware a user selects, so the browser build may carry them. The patch anchors (`devices/ports/`) are not embedded;
//! they come at run time (`apps/web/tests/patchkit.wasm.test.ts`).

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
        let _ = writeln!(
            s,
            "    ({n:?}, include_str!({:?})),",
            p.canonicalize().unwrap_or(p).to_string_lossy()
        );
    }
    s.push_str("];\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("device_files.rs");
    std::fs::write(out, s).unwrap();
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

/// The `path = "..."` dependencies (normal, build and target tables, not dev-dependencies; optional ones only with the feature `record-rules`) of the crate in `dir`, as canonical directories.
fn path_dependencies(dir: &std::path::Path) -> Vec<PathBuf> {
    let toml = dir.join("Cargo.toml");
    println!("cargo:rerun-if-changed={}", toml.display());
    let text = std::fs::read_to_string(&toml).unwrap_or_default();
    let mut live = false;
    // optional dependencies (patchkit's `record-rules` services) are sources of this build only with the feature
    let optional_deps_built = std::env::var_os("CARGO_FEATURE_RECORD_RULES").is_some();
    let mut out = vec![];
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            live = t.contains("dependencies") && !t.contains("dev-dependencies");
        } else if live
            && (optional_deps_built || !t.contains("optional = true"))
            && let Some(at) = t.find("path = \"")
        {
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
