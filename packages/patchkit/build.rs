//! Embeds the device files (`devices/*.toml`) so that the device list is data: a new device file is one more entry here after a
//! rebuild, with no change to the Rust sources. The file list is written to `$OUT_DIR/device_files.rs` (see `src/devdata.rs`).
//! The files are embedded whole, `[[firmware]]` tables included: the web app ships device files with their stock-image hashes to
//! identify the firmware a user selects, so the browser build may carry them. The patch anchors (`devices/ports/`) are not embedded;
//! they come at run time (`apps/web/tests/patchkit.test.mjs`).

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

/// `PATCHKIT_SRC_ID`: a hash of the crate's sources (FNV-1a over the files in name order). A cache of patched images keys on it
/// (`patchkit::SRC_ID`), so a change to the linker or the apply code never serves an image made before it.
fn src_id() {
    let src = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("src");
    println!("cargo:rerun-if-changed={}", src.display());
    let mut files = vec![];
    let mut dirs = vec![src];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
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
