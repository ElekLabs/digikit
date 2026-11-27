// Native kernels for hot functions (src/cpu/native.rs). CF_NATIVE_DIR names a directory with the `kernels.rs` that
// `cfaot` wrote (tools: `cargo run -p coldfire --bin cfaot`). The generated code is firmware-derived, so it lives under
// out/ and is never committed (the same rule as SHARC_GEN_DIR). Without the variable the crate has no kernels.
use std::{env, path::Path};

fn main() {
    println!("cargo:rustc-check-cfg=cfg(cf_native)");
    println!("cargo:rerun-if-env-changed=CF_NATIVE_DIR");
    let Ok(dir) = env::var("CF_NATIVE_DIR") else {
        return;
    };
    let file = Path::new(&dir).join("kernels.rs");
    if !file.is_file() {
        panic!("CF_NATIVE_DIR={dir} has no kernels.rs");
    }
    let file = file.canonicalize().expect("kernels.rs path");
    println!("cargo:rustc-cfg=cf_native");
    println!("cargo:rustc-env=CF_NATIVE_FILE={}", file.display());
    println!("cargo:rerun-if-changed={}", file.display());
}
