fn main() {
    tauri_build::build();

    // Tauri embeds this in dev mode and merges it into .app bundles. The
    // workbench launcher also runs the unbundled custom-protocol executable.
    println!("cargo:rerun-if-changed=Info.plist");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") && !tauri_build::is_dev() {
        let path = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
            .join("Info.plist");
        println!(
            "cargo:rustc-link-arg-bin=digiemu-desktop=-Wl,-sectcreate,__TEXT,__info_plist,{}",
            path.display()
        );
    }
}
