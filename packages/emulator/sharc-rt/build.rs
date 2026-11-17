// The header of a core library names the sharc-rt source it was built from (`src/header.rs`): the
// SHA-256 of every `.rs` file under src/ plus Cargo.toml and build.rs (relative path, NUL, bytes, NUL,
// sorted), the rustc version, the target and the build flags that change code generation or layout (not the PGO-only flags, `src/pgo_flags.rs`). The hash code is the crate's own `src/sha256.rs`.
#[path = "src/pgo_flags.rs"]
mod pgo_flags;
#[path = "src/sha256.rs"]
mod sha256;

use std::path::{Path, PathBuf};
use std::process::Command;

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read src") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let src = root.join("src");
    println!("cargo:rerun-if-changed={}", src.display());
    let mut list = Vec::new();
    files(&src, &mut list);
    list.push(root.join("build.rs"));
    list.push(root.join("Cargo.toml"));
    list.sort();
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=build.rs");
    let mut h = sha256::Sha256::new();
    for path in &list {
        let rel = path.strip_prefix(&root).expect("under the manifest dir");
        h.update(rel.to_string_lossy().replace('\\', "/").as_bytes());
        h.update(&[0]);
        h.update(&std::fs::read(path).expect("read source"));
        h.update(&[0]);
    }
    let hex: String = h.finish().iter().map(|b| format!("{b:02x}")).collect();
    println!("cargo:rustc-env=SHARC_RT_SRC_SHA256={hex}");
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let out = Command::new(rustc)
        .arg("-vV")
        .output()
        .expect("run rustc -vV");
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next().unwrap_or("rustc").to_string();
    println!("cargo:rustc-env=SHARC_RT_RUSTC={line}");
    println!(
        "cargo:rustc-env=SHARC_RT_TARGET={}",
        std::env::var("TARGET").expect("target")
    );
    let var = |name: &str| std::env::var(name).unwrap_or_default();
    // PGO changes optimisation, not layout or ABI: a host built with a profile accepts a plain library and the other way
    // round, so every flag that only steers PGO stays out of the hash (src/pgo_flags.rs).
    let kept = pgo_flags::strip(&var("CARGO_ENCODED_RUSTFLAGS"));
    let mut f = sha256::Sha256::new();
    f.update(kept.as_bytes());
    let rustflags: String = f
        .finish()
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect();
    println!(
        "cargo:rustc-env=SHARC_RT_FLAGS=panic={} opt={} profile={} debug={} rustflags={rustflags}",
        var("CARGO_CFG_PANIC"),
        var("OPT_LEVEL"),
        var("PROFILE"),
        var("DEBUG")
    );
    for name in ["CARGO_ENCODED_RUSTFLAGS", "PROFILE", "OPT_LEVEL", "DEBUG"] {
        println!("cargo:rerun-if-env-changed={name}");
    }
}
