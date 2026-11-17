// Generated code lives under out/ (the block code is firmware-derived),
// never under native/, and is compiled by packages/emulator/sharc-gen. This crate only
// needs to know whether SHARC_GEN_DIR names one, to match sharc-gen's cfgs
// (sharc_gen, sharc_image), which only gate the tests and examples that use the generated
// modules directly: the engine reads its core through `St::core` (`sharc_rt::GenCore`).
// The files themselves are not tracked here, so editing the engine never depends on them.
use std::env;
use std::fmt::Write as _;
use std::path::Path;

/// The RECIPS and RSQRTS seed tables for the fast tier, written to
/// `$OUT_DIR/seed_tables.rs` from the core's own source
/// (`tools/sharc_dev/sharc_core/seed_tables.py`), so there is no second copy.
fn seed_tables() {
    let root = Path::new(&env::var("CARGO_MANIFEST_DIR").expect("manifest dir")).join("../../..");
    let src = root.join("tools/sharc_dev/sharc_core/seed_tables.py");
    println!("cargo:rerun-if-changed={}", src.display());
    let text = std::fs::read_to_string(&src).unwrap_or_else(|e| panic!("{}: {e}", src.display()));
    let mut out = String::new();
    for name in ["RECIPS_SEED", "RSQRTS_SEED"] {
        let head = format!("{name} = (");
        let at = text
            .find(&head)
            .unwrap_or_else(|| panic!("{name} not in seed_tables.py"));
        let body = &text[at + head.len()..];
        let body = &body[..body.find(')').expect("table end")];
        let vals: Vec<u8> = body
            .split(',')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(|t| {
                u8::from_str_radix(t.trim_start_matches("0x").trim_start_matches("0X"), 16)
                    .expect("seed entry")
            })
            .collect();
        assert_eq!(vals.len(), 128, "{name} must have 128 entries");
        let _ = write!(out, "pub static {name}: [u8; 128] = {vals:?};\n");
    }
    let dest = Path::new(&env::var("OUT_DIR").expect("out dir")).join("seed_tables.rs");
    std::fs::write(dest, out).expect("write seed_tables.rs");
}

fn main() {
    seed_tables();
    println!("cargo:rustc-check-cfg=cfg(sharc_gen)");
    println!("cargo:rustc-check-cfg=cfg(sharc_image)");
    println!("cargo:rustc-check-cfg=cfg(region_bench_sol)");
    let var = "SHARC_GEN_DIR";
    println!("cargo:rerun-if-env-changed={var}");
    // The region bench's hand-written versions (firmware-derived, never in
    // the repository): compiled in only when this names an existing file.
    println!("cargo:rerun-if-env-changed=SHARC_REGIONBENCH_SOL");
    if let Ok(path) = env::var("SHARC_REGIONBENCH_SOL")
        && Path::new(&path).is_file()
    {
        println!("cargo:rustc-cfg=region_bench_sol");
        println!("cargo:rustc-env=SHARC_REGIONBENCH_SOL_PATH={path}");
        println!("cargo:rerun-if-changed={path}");
    }
    let Ok(dir) = env::var(var) else {
        return;
    };
    let dir = Path::new(&dir);
    for name in ["syms.rs", "tables.rs", "core_i.rs", "core_g.rs"] {
        if !dir.join(name).is_file() {
            panic!("{var}={} has no {name}", dir.display());
        }
    }
    let dir = dir.canonicalize().expect("generated core directory");
    println!("cargo:rustc-cfg=sharc_gen");
    if dir.join("image.rs").is_file() {
        println!("cargo:rustc-cfg=sharc_image");
    }
}
