//! Local firmware symbol sources as stock-keyed `dk-addrmap` dev maps.

use std::path::Path;

use dkaddrmap::{AddrMap, Entry, Key, Layer};
use serde::{Deserialize, Serialize};
use tauri_plugin_dialog::DialogExt;

pub(crate) mod catalog;
mod database;

use catalog::{ReviewIndex, read_review};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SourceKind {
    Cf,
    Sharc,
    Map,
    Catalog,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    review: Option<ReviewIndex>,
}

#[derive(Serialize)]
pub(crate) struct SymbolRelease {
    device: &'static str,
    firmware: &'static str,
    sha256: &'static str,
    defaults: SymbolDefaults,
}

#[derive(Serialize)]
struct SymbolDefaults {
    cf: String,
    sharc: String,
    catalog: String,
}

#[tauri::command]
pub(crate) fn symbols_releases() -> Result<Vec<SymbolRelease>, String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    let registry = device_profile::Registry::embedded().map_err(|e| e.to_string())?;
    let profiles = registry.devices().iter().filter_map(|device| {
        let acceptance = device.acceptance.as_ref()?;
        let port = PORTS.iter().find(|port| {
            port.device == device.short
                && port.firmware == acceptance.firmware
                && port.dsp.is_some()
        })?;
        let dir = root.join("out/firmware").join(port.id);
        Some(SymbolRelease {
            device: port.device,
            firmware: port.firmware,
            sha256: port.stock,
            defaults: SymbolDefaults {
                cf: dir.join("main/firmware.sqlite").display().to_string(),
                sharc: dir.join("dsp/firmware.sqlite").display().to_string(),
                catalog: dir.join("dsp/review.html").display().to_string(),
            },
        })
    });
    Ok(profiles.collect())
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
        SourceKind::Catalog => ("Open SHARC catalog", &["html", "htm"]),
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
    tauri::async_runtime::spawn_blocking(move || {
        let path = Path::new(&path);
        let repo = std::env::current_dir().ok();
        let mut maps = read_source(kind, path)?;
        if !matches!(kind, SourceKind::Catalog) {
            for map in &mut maps {
                map.review = find_review(kind, path, map, repo.as_deref());
            }
        }
        Ok(maps)
    })
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
        SourceKind::Catalog => read_catalog(path).map(|map| vec![map]),
    };
    result.map_err(|e| format!("{}: {e}", path.display()))
}

fn find_review(
    kind: SourceKind,
    path: &Path,
    map: &SymbolMap,
    repo: Option<&Path>,
) -> Option<ReviewIndex> {
    let by_source = match kind {
        SourceKind::Cf | SourceKind::Catalog => return None,
        SourceKind::Map => read_review(path, map.sha256),
        SourceKind::Sharc => read_review(&path.with_file_name("review.addrmap.json"), map.sha256)
            .or_else(|| {
                let sibling = path
                    .canonicalize()
                    .ok()?
                    .with_file_name("review.addrmap.json");
                read_review(&sibling, map.sha256)
            })
            .filter(|index| index.cpu == dkaddrmap::Cpu::Sharc),
    };
    by_source.or_else(|| {
        let review = repo?
            .join("out/firmware")
            .join(format!("{}-{}", map.device, map.firmware))
            .join("dsp/review.addrmap.json");
        read_review(&review, map.sha256).filter(|index| index.cpu == dkaddrmap::Cpu::Sharc)
    })
}

fn read_catalog(path: &Path) -> Result<SymbolMap, String> {
    let index_path = path.with_extension("addrmap.json");
    let path = path
        .parent()
        .and_then(|dir| dir.canonicalize().ok())
        .zip(path.file_name())
        .map(|(dir, name)| dir.join(name))
        .ok_or("Catalog file is missing")?;
    let mut map = read_map(&index_path).map_err(|e| format!("{}: {e}", index_path.display()))?;
    map.review = read_review(&index_path, map.sha256)
        .filter(|review| review.cpu == dkaddrmap::Cpu::Sharc && Path::new(&review.path) == path);
    if map.review.is_none() {
        return Err(
            "The SHARC catalog and its sibling address map must come from the same export.".into(),
        );
    }
    Ok(map)
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
        review: None,
    })
}
