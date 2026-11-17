// The header of a core library names the sharc-rt source it was built from (`src/header.rs`): the
// SHA-256 of every file under src/ (relative path, NUL, bytes, NUL, sorted), the rustc version and the
// target. The hash code is the crate's own `src/sha256.rs`.
#[path = "src/sha256.rs"]
mod sha256;

use std::path::{Path, PathBuf};
use std::process::Command;

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read src") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            files(&path, out);
        } else {
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
    list.sort();
    let mut h = sha256::Sha256::new();
    for path in &list {
        let rel = path.strip_prefix(&src).expect("under src");
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
}
