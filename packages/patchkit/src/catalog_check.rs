//! Whole-repository consistency checks that `patchkit catalog-check` runs after the manifests are read. Each one used to be a Rust test over the
//! repository; they are gate commands now, because they check committed sources against each other and not the behaviour of the code.
//!
//! - [`core_generation`]: generating the files of the core modules from the explicit catalog of their manifests gives the files of a full scan.
//! - [`abi_boundaries`]: the source hash of every service object and of the runtime object reads each Rust boundary file of `dkrt-abi`
//!   (`SVC_ABI_FILES`), or an edit of one would leave the committed objects "current" while the loader refuses them (E_SVC_ABI).
//! - [`record_owners`] (feature `record-rules`): every record kind of `dkneeds::KINDS` has an owner with a rule in `record_rules`.
use crate::build::rust;
use crate::error::*;
use crate::module::{Build, CORE_MODULE_ROOTS, Library};
use std::path::{Path, PathBuf};

/// The manifests of the crates whose objects share `dkrt-abi`: the service files and the runtime object.
const ABI_USERS: [&str; 4] = [
    "services/fx/dks/Cargo.toml",
    "services/machines/dks/Cargo.toml",
    "services/lfo/dks/Cargo.toml",
    "runtime/core/src/rs/Cargo.toml",
];

/// Generation from the explicit manifest catalog of the core modules equals generation from a full scan; the number of generated files compared.
pub fn core_generation(root: &Path) -> Result<usize> {
    // The first pass of `gen`: read the existing cache declarations, never check freshness or build.
    let legacy = Library::scan_repo_defined(root, true, Some(&[]), &[])?;
    let core: Vec<_> = legacy
        .modules
        .iter()
        .filter(|m| {
            CORE_MODULE_ROOTS
                .iter()
                .any(|r| m.dir.starts_with(root.join(r)))
        })
        .collect();
    if core.is_empty() {
        return Err(err!("no core module found under {}", root.display()));
    }
    let paths: Vec<_> = core.iter().map(|m| m.dir.join("module.toml")).collect();
    let ids: Vec<_> = core.iter().map(|m| m.id().to_string()).collect();
    // This also verifies that every required module is in the explicit core set.
    let mut explicit = Library::from_manifests(
        Build {
            root,
            only: Some(&[]),
            defines: &[],
        },
        &paths,
        true,
        &ids,
    )?;
    let models = crate::typed::Models::from_dir(&root.join("devices"))?;
    let facts = crate::devgen::Facts::from_dir(&root.join("devices"))?;
    explicit.lower(&models)?;
    let generate = |lib: &Library, id: &str| -> Result<Vec<(String, String)>> {
        let mut files = crate::modgen::generate(lib, &models, id)?;
        files.extend(crate::devgen::generate(lib, &facts, id)?);
        files.sort();
        Ok(files)
    };
    let mut count = 0;
    for id in &ids {
        let baseline = generate(&legacy, id)?;
        if baseline != generate(&explicit, id)? {
            return Err(err!(
                "core generation from the explicit manifest catalog differs from a full scan for {id}"
            ));
        }
        count += baseline.len();
    }
    Ok(count)
}

/// Every Rust boundary file of `dkrt-abi` is among the files that the source hash of each service object and of the runtime object reads.
pub fn abi_boundaries(root: &Path) -> Result<()> {
    for manifest in ABI_USERS {
        let files: Vec<PathBuf> = rust::crate_files(&root.join(manifest))?
            .into_iter()
            .map(|p| p.canonicalize().map_err(|e| err!("{}: {e}", p.display())))
            .collect::<Result<_>>()?;
        // The .rs files: a contract include (the stub slot offsets) reaches the Rust objects through `gen_svc_abi.rs` (`FILES_HASH`), which is one of them.
        for f in crate::sdkgen::SVC_ABI_FILES
            .iter()
            .filter(|f| f.ends_with(".rs"))
        {
            let path = root.join(f);
            let path = path
                .canonicalize()
                .map_err(|e| err!("{}: {e}", path.display()))?;
            if !files.contains(&path) {
                return Err(err!(
                    "{manifest}: the source hash does not read the boundary file {f}"
                ));
            }
        }
    }
    Ok(())
}

/// Record kinds that no service has a rule for, as `(kind number, owner)`: add the owner to `record_rules::owner_rule`.
#[cfg(all(feature = "record-rules", not(target_arch = "wasm32")))]
pub fn record_owners() -> Result<()> {
    let missing = crate::record_rules::unruled_kinds();
    if missing.is_empty() {
        return Ok(());
    }
    Err(err!(
        "record kinds without an owner rule: {}",
        missing
            .iter()
            .map(|(kind, owner)| format!(
                "{kind} (owner {owner:?}): add it to record_rules::owner_rule"
            ))
            .collect::<Vec<_>>()
            .join("; ")
    ))
}
