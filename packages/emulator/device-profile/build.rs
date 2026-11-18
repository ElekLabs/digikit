//! Embeds the device files (`devices/*.toml`) so that the device list is data: a new device file is picked up by a rebuild, with no
//! change to the Rust sources. The list is written to `$OUT_DIR/device_files.rs` (see `EMBEDDED` in `src/lib.rs`). A file without a
//! `[device]` table (the LED maps) is not a device file and is left out.

use std::fmt::Write;
#[path = "src/device_header.rs"]
mod device_header;
use std::path::PathBuf;

fn main() {
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../../devices");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("devices/ is next to packages/")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".toml"))
        .collect();
    names.sort();
    let mut out = String::from("const EMBEDDED: &[(&str, &str)] = &[\n");
    for name in names {
        let path = dir.join(&name);
        println!("cargo:rerun-if-changed={}", path.display());
        let text = std::fs::read_to_string(&path).unwrap();
        if !text.lines().any(device_header::is_device_header) {
            continue;
        }
        let path = path.canonicalize().unwrap();
        let _ = writeln!(
            out,
            "    ({name:?}, include_str!({:?})),",
            path.to_string_lossy()
        );
    }
    out.push_str("];\n");
    let file = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("device_files.rs");
    std::fs::write(file, out).unwrap();
}
