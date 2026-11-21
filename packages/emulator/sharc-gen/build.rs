// The generated-code crate. Generated code lives under out/ (the block code is firmware-derived),
// never under native/. SHARC_GEN_DIR names a directory written by
// tools/sharc_dev/sharc_transpile.py (syms.rs, tables.rs, core_i.rs, core_g.rs) and
// optionally tools/sharc_dev/sharc_rsgen.py (image.rs and the block files it
// includes). Without it the crate builds the runtime only. Hosts do not link this crate: each device's core is a
// library (packages/emulator/sharc-core-lib), one build per SHARC_GEN_DIR.
use std::env;
use std::path::Path;

/// The variable that names the generated core directory.
const GEN_DIR_VAR: &str = "SHARC_GEN_DIR";

fn main() {
    println!("cargo:rustc-check-cfg=cfg(sharc_gen)");
    println!("cargo:rustc-check-cfg=cfg(sharc_image)");
    println!("cargo:rustc-check-cfg=cfg(sharc_gen_version)");
    println!("cargo:rustc-check-cfg=cfg(sharc_region_entries)");
    println!("cargo:rustc-check-cfg=cfg(sharc_block_bounds)");
    // The suffix of the environment variables with this core's fast-tier address lists: none. A host sets the
    // suffix of the device of a loaded core (`sharc_native::core_lib::load`).
    println!("cargo:rustc-env=SHARC_GEN_ENV_SUFFIX=");
    let var = GEN_DIR_VAR;
    println!("cargo:rerun-if-env-changed={var}");
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
        // Generated since REGION_ENTRIES: blocks can be barred per region.
        let image = std::fs::read_to_string(dir.join("image.rs")).unwrap_or_default();
        if image.contains("REGION_ENTRIES") {
            println!("cargo:rustc-cfg=sharc_region_entries");
        }
        if image.contains("pub static BLOCK_BOUNDS") {
            println!("cargo:rustc-cfg=sharc_block_bounds");
        }
    }
    // Generated before the staleness guard: no GENERATOR_VERSION, reported
    // as version 0 (build_info), which the loaders refuse.
    let tables = std::fs::read_to_string(dir.join("tables.rs")).expect("tables.rs");
    if tables.contains("pub const GENERATOR_VERSION") {
        println!("cargo:rustc-cfg=sharc_gen_version");
    }
    println!("cargo:rustc-env=SHARC_GEN_DIR={}", dir.display());
    println!("cargo:rerun-if-changed={}", dir.display());
    for entry in std::fs::read_dir(&dir).expect("read SHARC_GEN_DIR") {
        let path = entry.expect("dir entry").path();
        if path.extension().is_some_and(|e| e == "rs") {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}
