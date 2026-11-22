fn main() {
    tauri_build::build();
    symbol_port_fingerprints();

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

/// Embed only the stock and section fingerprints needed to identify local symbol sources.
fn symbol_port_fingerprints() {
    use std::{fmt::Write, path::PathBuf};

    let ports = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../ports");
    println!("cargo:rerun-if-changed={}", ports.display());
    let mut paths: Vec<_> = std::fs::read_dir(&ports)
        .expect("open ports directory")
        .map(|entry| entry.expect("read ports directory").path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    paths.sort();
    let mut source = String::from("const PORTS: &[PortFingerprint] = &[\n");
    for path in paths {
        println!("cargo:rerun-if-changed={}", path.display());
        let text = std::fs::read_to_string(&path).expect("read port file");
        let doc: toml::Table = text.parse().expect("parse port file");
        let port = &doc["port"];
        let field = |key: &str| port[key].as_str().expect("port field is text");
        let (device, firmware, stock, main) = (
            field("device"),
            field("version"),
            field("syx_sha256"),
            field("main_sha256"),
        );
        let dsp = port.get("dsp_sha256").and_then(toml::Value::as_str);
        writeln!(
            source,
            "    PortFingerprint {{ id: {id:?}, device: {device:?}, firmware: {firmware:?}, stock: {stock:?}, main: {main:?}, dsp: {dsp:?} }},",
            id = format!("{device}-{firmware}"),
        )
        .unwrap();
    }
    source.push_str("];\n");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    std::fs::write(out.join("symbol_ports.rs"), source).expect("write port fingerprints");
}
