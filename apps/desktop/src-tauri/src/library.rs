//! The module library: the built `.dkm` files the Transfer page offers. The app does not build or pack; a dev task (`mise run
//! module:pack-all`, run by `mise run workbench`) writes one folder per device, `out/modules/<device>/<id>.dkm`, and this file lets
//! the page read it (the one thing the page cannot do itself: file access). The folder is the single constant below; a packaged app
//! would bundle a folder and point it here.
use std::path::{Path, PathBuf};

use serde::Serialize;

/// The library folder, relative to the repository root (one subfolder per device short name).
const LIBRARY_DIR: &str = "out/modules";

#[derive(Serialize, Debug, PartialEq)]
pub struct LibFile {
    /// Relative to the device's folder, `/` separated (`<author/name>.dkm`).
    pub path: String,
    pub size: u64,
}

fn device_dir(root: &Path, device: &str) -> Result<PathBuf, String> {
    if device.is_empty()
        || !device
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    {
        return Err(format!("not a device id: {device:?}"));
    }
    Ok(root.join(LIBRARY_DIR).join(device))
}

fn walk(dir: &Path, base: &Path, out: &mut Vec<LibFile>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let meta = entry.metadata()?;
        if meta.is_dir() {
            walk(&path, base, out)?;
        } else if path.extension().is_some_and(|e| e == "dkm") {
            let rel = path.strip_prefix(base).unwrap_or(&path);
            let rel = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            out.push(LibFile {
                path: rel,
                size: meta.len(),
            });
        }
    }
    Ok(())
}

/// The `.dkm` files under the device's folder, by path. A missing folder is an empty library.
pub fn scan(root: &Path, device: &str) -> Result<Vec<LibFile>, String> {
    let dir = device_dir(root, device)?;
    let mut out = vec![];
    match walk(&dir, &dir, &mut out) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{}: {e}", dir.display())),
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// The bytes of one library file; a path that leaves the device's folder is refused.
pub fn read(root: &Path, device: &str, rel: &str) -> Result<Vec<u8>, String> {
    let dir = device_dir(root, device)?;
    if rel
        .split('/')
        .any(|p| p.is_empty() || p == "." || p == "..")
        || !rel.ends_with(".dkm")
    {
        return Err(format!("not a library file: {rel:?}"));
    }
    let path = dir.join(rel);
    std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
}

fn root() -> Result<PathBuf, String> {
    digi_control::runtime_patch::find_root(None)
}

/// The built modules of a device (its short name): path and size of each.
#[tauri::command]
pub async fn lib_scan(device: String) -> Result<Vec<LibFile>, String> {
    tauri::async_runtime::spawn_blocking(move || scan(&root()?, &device))
        .await
        .map_err(|e| e.to_string())?
}

/// One library file as raw bytes.
#[tauri::command]
pub async fn lib_read(device: String, path: String) -> Result<tauri::ipc::Response, String> {
    tauri::async_runtime::spawn_blocking(move || {
        read(&root()?, &device, &path).map(tauri::ipc::Response::new)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The converter of the Samples tab (`apps/web/public/dksample.wasm`, built by `mise run wasm:patchkit`),
/// as raw bytes. The page cannot import it: the file is a build product, not in the repository.
#[tauri::command]
pub async fn sample_wasm() -> Result<tauri::ipc::Response, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let path = root()?.join("apps/web/public/dksample.wasm");
        std::fs::read(&path)
            .map(tauri::ipc::Response::new)
            .map_err(|e| format!("{}: {e} (build it: mise run wasm:patchkit)", path.display()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The needs reader of the Transfer page: `apps/web/public/patchkit.wasm` (`pk_needs`, the Rust code that infers what a `.dkm` takes of the device;
/// built by `mise run wasm:patchkit`), as raw bytes.
#[tauri::command]
pub async fn patchkit_wasm() -> Result<tauri::ipc::Response, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let path = root()?.join("apps/web/public/patchkit.wasm");
        std::fs::read(&path)
            .map(tauri::ipc::Response::new)
            .map_err(|e| format!("{}: {e} (build it: mise run wasm:patchkit)", path.display()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// What the device profile says about content (`[storage]`): the device's name and the backend of its samples.
#[derive(Serialize, Debug, PartialEq)]
pub struct DeviceStorage {
    pub name: String,
    pub samples: String,
}

/// The `[storage]` of a device (its short name) from `devices/*.toml`.
pub fn storage_of(device: &str) -> Result<DeviceStorage, String> {
    let registry = device_profile::Registry::load().map_err(|e| e.to_string())?;
    let profile = registry
        .devices()
        .iter()
        .find(|d| d.short == device)
        .ok_or_else(|| format!("no device profile for {device:?}"))?;
    let storage = profile
        .storage
        .as_ref()
        .ok_or_else(|| format!("the {} profile has no [storage]", profile.name))?;
    Ok(DeviceStorage {
        name: profile.name.clone(),
        samples: storage.samples.clone(),
    })
}

/// Where the device keeps samples: read from its profile, so the page names no device.
#[tauri::command]
pub async fn device_storage(device: String) -> Result<DeviceStorage, String> {
    tauri::async_runtime::spawn_blocking(move || storage_of(&device))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The short names of the first two devices of the registry (a device with a library, and one without).
    fn two_devices() -> (String, String) {
        let registry = device_profile::Registry::load().unwrap();
        let mut shorts = registry.devices().iter().map(|d| d.short.clone());
        (shorts.next().unwrap(), shorts.next().unwrap())
    }

    fn library(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("digi-library-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join(LIBRARY_DIR).join(two_devices().0);
        std::fs::create_dir_all(dir.join("acme")).unwrap();
        std::fs::create_dir_all(dir.join("test")).unwrap();
        std::fs::write(dir.join("acme/synth.dkm"), b"SYNTH").unwrap();
        std::fs::write(dir.join("test/sine.dkm"), b"SINE!!").unwrap();
        std::fs::write(dir.join("notes.txt"), b"not a module").unwrap();
        root
    }

    #[test]
    fn the_scan_lists_dkm_files_by_path_and_a_missing_folder_is_empty() {
        let root = library("scan");
        let (full, empty) = two_devices();
        assert_eq!(
            scan(&root, &full).unwrap(),
            vec![
                LibFile {
                    path: "acme/synth.dkm".into(),
                    size: 5
                },
                LibFile {
                    path: "test/sine.dkm".into(),
                    size: 6
                }
            ]
        );
        assert_eq!(scan(&root, &empty).unwrap(), vec![]);
        assert!(scan(&root, "../x").is_err());
        assert_eq!(read(&root, &full, "acme/synth.dkm").unwrap(), b"SYNTH");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_storage_comes_from_the_profile() {
        let registry = device_profile::Registry::load().unwrap();
        let mut found = 0;
        for profile in registry.devices().iter().filter(|d| d.storage.is_some()) {
            let storage = storage_of(&profile.short).unwrap();
            assert_eq!(storage.name, profile.name);
            assert_eq!(
                Some(&storage.samples),
                profile.storage.as_ref().map(|s| &s.samples)
            );
            found += 1;
        }
        assert!(found > 0);
        assert!(storage_of("nothing").is_err());
    }

    #[test]
    fn a_read_stays_inside_the_device_folder() {
        let root = library("read");
        let (full, empty) = two_devices();
        let outside = format!("../{empty}/x.dkm");
        for bad in [
            outside.as_str(),
            "digikit/../../x.dkm",
            "/etc/passwd",
            "notes.txt",
            "",
            "a//b.dkm",
        ] {
            assert!(read(&root, &full, bad).is_err(), "{bad}");
        }
        assert!(read(&root, &full, "nothing.dkm").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
