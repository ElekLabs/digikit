//! `patchkit services`: which card services a set of modules needs, and the packed file of each. A service is a file of the card
//! (`services/<name>.dks`), as packed: it holds no firmware bytes, and the port file of the firmware resolves its firmware references. A module that needs one has a hard NEEDS item for its name hash in its header
//! table; the service of a name is the module whose `module.toml` has `role = "service"` and `service = NAME`. The device tests, `dk:dev` and
//! `module:check` all ask this one command.

use crate::dkm::Dkm;
use crate::error::*;
use crate::module::{LibrarySource, ModuleRole, library_with, selected_library};
use crate::pack::{PackOptions, pack_module};
use dkneeds::keys::{Keys, name_hash};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Service name to package id, from the manifests.
fn service_modules(root: &Path) -> Result<BTreeMap<String, String>> {
    // no objects are loaded: only the manifests are read
    let lib = library_with(root, &LibrarySource::default(), false, Some(&[]))?;
    let mut found = BTreeMap::<String, String>::new();
    let mut identities = BTreeMap::new();
    for m in &lib.modules {
        let module = &m.manifest.module;
        if module.role != Some(ModuleRole::Service) {
            continue;
        }
        let Some(name) = &module.service else {
            continue;
        };
        if let Some(other) = found.get(name) {
            return Err(err!("service {name} is provided by {other} and {}", m.id()));
        }
        if let Some(other) = identities.insert(name_hash(name), name) {
            return Err(err!(
                "services {other} and {name} have the same identity hash"
            ));
        }
        found.insert(name.clone(), m.id().to_string());
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

/// Add the services that a packed file hard-needs and this library provides.
fn add_needs(
    modules: &BTreeMap<String, String>,
    file: &[u8],
    pending: &mut BTreeSet<String>,
) -> Result<()> {
    let hashes = needed_hashes(file)?;
    pending.extend(
        modules
            .keys()
            .filter(|name| hashes.contains(&name_hash(name)))
            .cloned(),
    );
    Ok(())
}

/// Pack the service module `id` for `device`; its bytes give the actual hard needs for both listing and staging.
fn pack_service(root: &Path, id: &str, device: &str) -> Result<Vec<u8>> {
    let ids = [id.to_string()];
    let lib = selected_library(root, &LibrarySource::default(), false, &ids, &ids)?;
    let opts = PackOptions {
        device: Some(device.into()),
        ..Default::default()
    };
    Ok(pack_module(&lib, id, &opts)?.bytes)
}

/// The command line: see the usage text of `patchkit`.
pub fn run(root: &Path, rest: &[String]) -> Result<()> {
    let (mut device, mut dir) = (None, None);
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
            "--pack-dir" => dir = Some(PathBuf::from(value("--pack-dir")?)),
            "--module" => named.push(value("--module")?),
            "--service" => by_name.push(value("--service")?),
            f if f.starts_with('-') => return Err(err!("services: unknown flag {f}")),
            f => files.push(f.to_string()),
        }
    }
    let device = device.ok_or_else(|| err!("services: --device is required"))?;
    let modules = service_modules(root)?;
    let mut pending = BTreeSet::new();
    for id in &named {
        let (name, _) = modules
            .iter()
            .find(|(_, m)| *m == id)
            .ok_or_else(|| err!("{id} is not a service module (role = \"service\")"))?;
        pending.insert(name.clone());
    }
    for name in &by_name {
        if !modules.contains_key(name) {
            return Err(err!("no module provides the service {name}"));
        }
        pending.insert(name.clone());
    }
    for f in &files {
        let bytes = std::fs::read(f).map_err(|e| err!("{f}: {e}"))?;
        add_needs(&modules, &bytes, &mut pending)?;
    }
    // A call with only explicit names and no output directory is a manifest lookup (used to recognize service ids).
    // A dependency query or staging command visits each provider once, including cycles, using its packed NEEDS.
    let closure = dir.is_some() || !files.is_empty();
    let mut selected = BTreeMap::new();
    while let Some(name) = pending.pop_first() {
        if selected.contains_key(&name) {
            continue;
        }
        let mut file = None;
        if closure {
            let bytes = pack_service(root, &modules[&name], &device)?;
            add_needs(&modules, &bytes, &mut pending)?;
            if let Some(dir) = &dir {
                let path = dir.join(format!("{name}.dks"));
                std::fs::create_dir_all(dir).map_err(|e| err!("{}: {e}", dir.display()))?;
                std::fs::write(&path, &bytes).map_err(|e| err!("{}: {e}", path.display()))?;
                file = Some(path);
            }
        }
        selected.insert(name, file);
    }
    for (name, file) in selected {
        let mut line = serde_json::json!({
            "service": name,
            "module_id": modules[&name],
            "store_path": format!("services/{name}.dks"),
        });
        if let Some(file) = file {
            line["file"] = file.to_string_lossy().into();
        }
        println!("{line}");
    }
    Ok(())
}
