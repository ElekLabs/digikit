//! `patchkit services`: which card services a set of modules needs, and the bound file of each. A service with hooks is a file of the card
//! (`services/<name>.dks`), bound to the stock firmware of the device. A module that needs one has a hard NEEDS item for its name hash in its header
//! table; the service of a name is the module whose `module.toml` has `role = "service"` and `service = NAME`. The device tests, `dk:dev` and
//! `module:check` all ask this one command.

use crate::bind;
use crate::dkm::Dkm;
use crate::error::*;
use crate::module::{LibrarySource, ModuleRole, library_with, selected_library};
use crate::pack::{PackOptions, pack_module};
use dkneeds::keys::{Keys, name_hash};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Service name to module id, from the manifests.
pub fn service_modules(root: &Path) -> Result<BTreeMap<String, String>> {
    // no objects are loaded: only the manifests are read
    let lib = library_with(root, &LibrarySource::default(), false, Some(&[]))?;
    let mut found = BTreeMap::new();
    for m in &lib.modules {
        let module = &m.manifest.module;
        if module.role != Some(ModuleRole::Service) {
            continue;
        }
        if let Some(name) = &module.service
            && let Some(other) = found.insert(name.clone(), m.id().to_string())
        {
            return Err(err!("service {name} is provided by {other} and {}", m.id()));
        }
    }
    Ok(found)
}

/// The service name hashes of the hard NEEDS of a packed file.
pub fn needed_hashes(dkm: &[u8]) -> Result<Vec<u32>> {
    let d = Dkm::parse(dkm)?;
    if d.role == crate::dkm::ROLE_PORT {
        return Err(err!("{}: a port file is not a module or a service", d.name));
    }
    let keys = Keys::parse(&d.register).map_err(|e| err!("header table: {e:?}"))?;
    Ok(keys.needs().filter(|n| n.hard).map(|n| n.service).collect())
}

/// The services (name, module id) that the packed files need, in name order.
pub fn needed_services(
    modules: &BTreeMap<String, String>,
    files: &[Vec<u8>],
) -> Result<Vec<(String, String)>> {
    let mut hashes = vec![];
    for f in files {
        hashes.extend(needed_hashes(f)?);
    }
    Ok(modules
        .iter()
        .filter(|(name, _)| hashes.contains(&name_hash(name)))
        .map(|(n, id)| (n.clone(), id.clone()))
        .collect())
}

/// Pack the service module `id` for `device` and bind it to the stock image `firmware` as `dir/<name>.dks`. A file whose sidecar matches the fresh
/// pack and the image is kept.
fn bound(
    root: &Path,
    id: &str,
    name: &str,
    device: &str,
    firmware: &Path,
    dir: &Path,
) -> Result<PathBuf> {
    let ids = [id.to_string()];
    let lib = selected_library(root, &LibrarySource::default(), false, &ids, &ids)?;
    let opts = PackOptions {
        device: Some(device.into()),
        ..Default::default()
    };
    let packed = pack_module(&lib, id, &opts)?.bytes;
    let fw = std::fs::read(firmware).map_err(|e| err!("{}: {e}", firmware.display()))?;
    let out = dir.join(format!("{name}.dks"));
    if bind::stale_binding(&out, &packed, &sha256_hex(&fw)).is_none() {
        return Ok(out);
    }
    if !bind::output_allowed(root, &out) {
        return Err(err!(
            "{}: the bound file holds firmware bytes: write it under dist/ or out/ (or outside the source tree)",
            out.display()
        ));
    }
    let reg = crate::port::Registry::from_dir(&root.join("devices/ports"))?;
    let texts = crate::fwgen::fw_texts(root)?;
    let decls: Vec<&str> = texts.iter().map(String::as_str).collect();
    let (file, _hooks) = bind::bind(&packed, &fw, &reg, &decls)?;
    std::fs::create_dir_all(dir).map_err(|e| err!("{}: {e}", dir.display()))?;
    std::fs::write(&out, file).map_err(|e| err!("{}: {e}", out.display()))?;
    std::fs::write(bind::sidecar_path(&out), bind::sidecar_json(&packed, &fw))
        .map_err(|e| err!("{}: {e}", out.display()))?;
    Ok(out)
}

/// The command line: see the usage text of `patchkit`.
pub fn run(root: &Path, rest: &[String]) -> Result<()> {
    let (mut device, mut firmware, mut dir) = (None, None, None);
    let (mut named, mut by_name, mut files) = (vec![], vec![], vec![]);
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        let mut value = |what: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| err!("services: {what} needs a value"))
        };
        match a.as_str() {
            "--device" => device = Some(value("--device")?),
            "--firmware" => firmware = Some(PathBuf::from(value("--firmware")?)),
            "--bind-dir" => dir = Some(PathBuf::from(value("--bind-dir")?)),
            "--module" => named.push(value("--module")?),
            "--service" => by_name.push(value("--service")?),
            f if f.starts_with('-') => return Err(err!("services: unknown flag {f}")),
            f => files.push(f.to_string()),
        }
    }
    let device = device.ok_or_else(|| err!("services: --device is required"))?;
    let bind_to = match (&firmware, &dir) {
        (Some(f), Some(d)) => Some((f, d)),
        (None, None) => None,
        _ => return Err(err!("services: --firmware and --bind-dir go together")),
    };
    let modules = service_modules(root)?;
    let mut wanted: BTreeMap<String, String> = BTreeMap::new();
    for id in &named {
        let (name, _) = modules
            .iter()
            .find(|(_, m)| *m == id)
            .ok_or_else(|| err!("{id} is not a service module (role = \"service\")"))?;
        wanted.insert(name.clone(), id.clone());
    }
    for name in &by_name {
        let id = modules
            .get(name)
            .ok_or_else(|| err!("no module provides the service {name}"))?;
        wanted.insert(name.clone(), id.clone());
    }
    let bytes = files
        .iter()
        .map(|f| std::fs::read(f).map_err(|e| err!("{f}: {e}")))
        .collect::<Result<Vec<_>>>()?;
    wanted.extend(needed_services(&modules, &bytes)?);
    for (name, id) in &wanted {
        let mut line = serde_json::json!({
            "service": name,
            "module_id": id,
            "store_path": format!("services/{name}.dks"),
        });
        if let Some((fw, dir)) = bind_to {
            line["bound_path"] = bound(root, id, name, &device, fw, dir)?
                .to_string_lossy()
                .into();
        }
        println!("{line}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_or_foreign_sidecar_is_stale() {
        let dir = std::env::temp_dir().join(format!("pk-stale-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("fx.dks");
        let (packed, fw) = (b"packed".as_slice(), sha256_hex(b"image"));
        assert!(
            bind::stale_binding(&out, packed, &fw)
                .unwrap()
                .contains("missing")
        );
        std::fs::write(&out, b"bound").unwrap();
        assert!(
            bind::stale_binding(&out, packed, &fw)
                .unwrap()
                .contains("no .src.json")
        );
        std::fs::write(
            bind::sidecar_path(&out),
            bind::sidecar_json(packed, b"image"),
        )
        .unwrap();
        assert_eq!(bind::stale_binding(&out, packed, &fw), None);
        assert!(
            bind::stale_binding(&out, b"packed again", &fw)
                .unwrap()
                .contains("another packed")
        );
        assert!(
            bind::stale_binding(&out, packed, &sha256_hex(b"other"))
                .unwrap()
                .contains("another firmware")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
