//! Run the checkout's offline rig checks and keep the report and command log together.
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use patchkit::module::{LibrarySource, ModuleKind, library_with};
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize)]
pub struct DspQualityRun {
    folder: PathBuf,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct DspQualityModule {
    id: String,
    name: String,
}

#[tauri::command]
pub async fn dsp_quality_modules() -> Result<Vec<DspQualityModule>, String> {
    tauri::async_runtime::spawn_blocking(|| modules(&digi_control::runtime_patch::find_root(None)?))
        .await
        .map_err(|e| e.to_string())?
}

fn modules(root: &Path) -> Result<Vec<DspQualityModule>, String> {
    let library = library_with(root, &LibrarySource::default(), false, Some(&[]))
        .map_err(|e| e.to_string())?;
    let mut choices = Vec::new();
    for module in library.modules {
        let info = module.manifest.module;
        if !matches!(info.kind, ModuleKind::Machine | ModuleKind::Fx)
            || info.category == "test"
            || info.id.starts_with("test/")
            || !valid_module_id(&info.id)
        {
            continue;
        }
        let dir = module.dir.join("tests/rig");
        if has_rig_cases(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            choices.push(DspQualityModule {
                id: info.id,
                name: info.name,
            });
        }
    }
    choices.sort_by_cached_key(|module| (module.name.to_lowercase(), module.id.clone()));
    Ok(choices)
}

fn has_rig_cases(dir: &Path) -> io::Result<bool> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "toml") && path.is_file() {
            return Ok(true);
        }
    }
    Ok(false)
}

#[tauri::command]
pub async fn dsp_quality_run(module: String) -> Result<DspQualityRun, String> {
    if !valid_module_id(&module) {
        return Err("Choose a lowercase module ID in author/name form.".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let root = digi_control::runtime_patch::find_root(None)?;
        let root = fs::canonicalize(&root).map_err(|e| format!("{}: {e}", root.display()))?;
        run(&root, &module)
    })
    .await
    .map_err(|e| e.to_string())?
}

pub(crate) fn valid_module_id(module: &str) -> bool {
    let valid_part = |part: &str| {
        !part.is_empty()
            && !part.starts_with('-')
            && part
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'))
    };
    module
        .split_once('/')
        .is_some_and(|(author, name)| valid_part(author) && valid_part(name))
}

fn reserve_folder(root: &Path) -> Result<PathBuf, String> {
    let runs = Path::new("out/runs");
    for relative in [Path::new("out"), runs] {
        let dir = root.join(relative);
        match fs::create_dir(&dir) {
            Ok(()) => (),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(format!("{}: {e}", dir.display())),
        }
        let resolved = fs::canonicalize(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        if !resolved.starts_with(root) || !resolved.is_dir() {
            return Err(format!(
                "{} must be a directory inside the checkout.",
                dir.display()
            ));
        }
    }
    let folder = tempfile::Builder::new()
        .prefix("dsp-quality-")
        .tempdir_in(root.join(runs))
        .map_err(|e| format!("Could not reserve a DSP quality report folder: {e}"))?
        .keep();
    Ok(runs.join(folder.file_name().expect("temporary directory name")))
}

fn run(root: &Path, module: &str) -> Result<DspQualityRun, String> {
    let relative = reserve_folder(root)?;
    let folder = root.join(&relative);
    let log_path = folder.join("run.log");
    let log = File::create_new(&log_path).map_err(|e| format!("{}: {e}", log_path.display()))?;
    let stderr = log
        .try_clone()
        .map_err(|e| format!("{}: {e}", log_path.display()))?;
    let status = Command::new("mise")
        .args(["run", "wb:cli", "--", "module", "quality", module, "--emit", "json,html", "--out"])
        .arg(&relative)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(stderr)
        .status()
        .map_err(|e| {
            format!(
                "Could not start mise in {}: {e}. Check that mise is installed and on Workbench's PATH.",
                root.display()
            )
        })?;

    // The CLI writes the report and exits 1 when the rig checks fail.
    if let Err(e) = check_report(&folder, module) {
        let mut message = format!(
            "The rig checks did not produce a usable report ({status}): {e}\nLog: {}",
            log_path.display()
        );
        if let Ok(tail) = log_tail(&log_path)
            && !tail.trim().is_empty()
        {
            message.push_str("\n");
            message.push_str(tail.trim());
        }
        return Err(message);
    }
    Ok(DspQualityRun { folder })
}

fn check_report(folder: &Path, module: &str) -> Result<(), String> {
    let data = fs::read(folder.join("quality.json")).map_err(|e| format!("quality.json: {e}"))?;
    parse_report(&data, module).map(|_| ())
}

pub(crate) fn parse_report(data: &[u8], module: &str) -> Result<Value, String> {
    let report: Value = serde_json::from_slice(data).map_err(|e| format!("quality.json: {e}"))?;
    if report["schema"] == "dk.module-quality/1"
        && report["module"] == module
        && matches!(report["outcome"].as_str(), Some("pass" | "fail"))
    {
        Ok(report)
    } else {
        Err(format!(
            "quality.json is not a module quality report for {module}."
        ))
    }
}

fn log_tail(path: &Path) -> io::Result<String> {
    const TAIL_BYTES: u64 = 8 * 1024;
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(
        file.metadata()?.len().saturating_sub(TAIL_BYTES),
    ))?;
    let mut bytes = Vec::new();
    file.take(TAIL_BYTES).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests;
