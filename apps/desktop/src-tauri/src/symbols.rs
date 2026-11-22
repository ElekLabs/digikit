//! Local firmware symbol sources as stock-keyed `dk-addrmap` dev maps.

use std::path::Path;

use dkaddrmap::{AddrMap, Entry, Key, Layer};
use serde::{Deserialize, Serialize};
use tauri_plugin_dialog::DialogExt;

mod database;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SourceKind {
    Cf,
    Sharc,
    Map,
}

struct PortFingerprint {
    id: &'static str,
    device: &'static str,
    firmware: &'static str,
    stock: &'static str,
    main: &'static str,
    dsp: Option<&'static str>,
}

include!(concat!(env!("OUT_DIR"), "/symbol_ports.rs"));

#[derive(Serialize)]
pub(crate) struct SymbolMap {
    device: &'static str,
    firmware: &'static str,
    sha256: &'static str,
    json: String,
}

#[tauri::command]
pub(crate) async fn symbols_pick(
    app: tauri::AppHandle,
    kind: SourceKind,
) -> Result<Option<String>, String> {
    let (title, extensions): (&str, &[&str]) = match kind {
        SourceKind::Cf => ("Open ColdFire database", &["sqlite", "sqlite3", "db"]),
        SourceKind::Sharc => ("Open SHARC database", &["sqlite", "sqlite3", "db"]),
        SourceKind::Map => ("Open developer address map", &["json"]),
    };
    let dialog = app
        .dialog()
        .file()
        .set_title(title)
        .add_filter(title, extensions);
    let file = tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_file())
        .await
        .map_err(|e| e.to_string())?;
    file.map(|file| {
        file.into_path()
            .map_err(|e| e.to_string())?
            .into_os_string()
            .into_string()
            .map_err(|_| "symbol source path is not valid Unicode".to_string())
    })
    .transpose()
}

#[tauri::command]
pub(crate) async fn symbols_read(kind: SourceKind, path: String) -> Result<Vec<SymbolMap>, String> {
    tauri::async_runtime::spawn_blocking(move || read_source(kind, Path::new(&path)))
        .await
        .map_err(|e| e.to_string())?
}

fn read_source(kind: SourceKind, path: &Path) -> Result<Vec<SymbolMap>, String> {
    let metadata = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("{}: select a symbol source file", path.display()));
    }
    let result = match kind {
        SourceKind::Cf => database::read(path, dkaddrmap::Cpu::Cf),
        SourceKind::Sharc => database::read(path, dkaddrmap::Cpu::Sharc),
        SourceKind::Map => read_map(path).map(|map| vec![map]),
    };
    result.map_err(|e| format!("{}: {e}", path.display()))
}

fn read_map(path: &Path) -> Result<SymbolMap, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let map = AddrMap::parse(&text)?;
    if map.kind != "dev" {
        return Err("expected a developer address map (kind dev)".into());
    }
    let (Some(device), Some(firmware)) = (&map.key.device, &map.key.firmware) else {
        return Err("developer address map is missing its device or firmware version".into());
    };
    let port = unique_port(|p| p.device == device && p.firmware == firmware)?
        .ok_or_else(|| format!("unknown firmware {device}-{firmware}"))?;
    verify_hash(map.key.image_sha256.as_deref(), port.stock, port.id)?;
    make_map(port, map.entries)
}

fn unique_port(
    mut matches: impl FnMut(&PortFingerprint) -> bool,
) -> Result<Option<&'static PortFingerprint>, String> {
    let mut ports = PORTS.iter().filter(|port| matches(port));
    let port = ports.next();
    if ports.next().is_some() {
        return Err("symbol source matches multiple firmware ports".into());
    }
    Ok(port)
}

fn verify_hash(hash: Option<&str>, expected: &str, image: &str) -> Result<(), String> {
    let hash = hash.ok_or_else(|| format!("{image}: missing image SHA-256 fingerprint"))?;
    let hash = hash.trim();
    if hash.len() != 64 || !hash.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("{image}: invalid image SHA-256 fingerprint"));
    }
    if !hash.eq_ignore_ascii_case(expected) {
        return Err(format!(
            "{image}: image SHA-256 does not match the firmware port"
        ));
    }
    Ok(())
}

fn make_map(port: &PortFingerprint, entries: Vec<Entry>) -> Result<SymbolMap, String> {
    if entries.is_empty() {
        return Err(format!("{}: no firmware symbol entries", port.id));
    }
    for entry in &entries {
        if entry.layer != Layer::Fw || entry.module.is_some() {
            return Err("developer address maps require absolute addresses in the fw layer".into());
        }
        if entry.addr > u64::from(u32::MAX)
            || entry.size > u64::from(u32::MAX) + 1 - entry.addr
            || entry.name.trim().is_empty()
        {
            return Err(format!(
                "{}: invalid firmware symbol at {:#x}",
                port.id, entry.addr
            ));
        }
    }
    let key = Key {
        device: Some(port.device.into()),
        firmware: Some(port.firmware.into()),
        image_sha256: Some(port.stock.into()),
        ..Key::default()
    };
    Ok(SymbolMap {
        device: port.device,
        firmware: port.firmware,
        sha256: port.stock,
        json: AddrMap::new("dev", key, entries).to_json(),
    })
}
