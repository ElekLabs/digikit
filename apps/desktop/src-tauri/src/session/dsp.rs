//! Copy the report and its portable listening evidence before publishing the session.
use super::{
    bundle::{OpenSession, read},
    document::{atomic, problem},
};
use crate::dsp_quality::{parse_report, valid_module_id};
use digi_control::firmware::sha256_hex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

const REPORT: &str = "quality.json";
const MEDIA: &str = "quality-media";
const MIB: u64 = 1024 * 1024;
const MAX_BUNDLE: u64 = 256 * MIB;
const MAX_MEDIA_FILES: usize = 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Dsp {
    pub(super) module: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) report_folder: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct ReportSource {
    pub(super) folder: PathBuf,
    pub(super) sha256: String,
}

impl Dsp {
    pub(super) fn valid(&self) -> bool {
        valid_module_id(&self.module)
            && self
                .report_folder
                .as_ref()
                .is_none_or(|folder| folder.strip_prefix("dsp/").is_some_and(hex_digest))
    }

    pub(super) fn selection(&self) -> Self {
        Self {
            module: self.module.clone(),
            report_folder: None,
        }
    }
}

fn hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

struct MediaKind {
    extension: &'static str,
    stems: &'static [&'static str],
    limit: u64,
}

const JSON: MediaKind = MediaKind {
    extension: ".json",
    stems: &["comparison"],
    limit: 4 * MIB,
};
const WAV: MediaKind = MediaKind {
    extension: ".wav",
    stems: &["audio"],
    limit: 32 * MIB,
};
const PNG: MediaKind = MediaKind {
    extension: ".png",
    stems: &["left", "right", "waveform", "plot"],
    limit: 4 * MIB,
};

fn media_name<'a>(path: &'a str, kind: &MediaKind) -> Result<Option<(&'a str, &'a str)>, String> {
    let Some(name) = path.strip_prefix("quality-media/") else {
        // Legacy links are kept in the original report for display only.
        return Ok(None);
    };
    let digest = name
        .strip_suffix(kind.extension)
        .and_then(|stem| stem.split_once('-'))
        .filter(|(stem, digest)| kind.stems.contains(stem) && hex_digest(digest))
        .map(|(_, digest)| digest)
        .ok_or_else(|| format!("invalid portable report path: {path}"))?;
    Ok(Some((name, digest)))
}

fn media_refs<'a>(report: &'a Value) -> Result<BTreeSet<(&'a str, &'a str, u64)>, String> {
    let mut refs = BTreeSet::new();
    let mut add = |path: &'a Value, kind: &MediaKind| -> Result<(), String> {
        if let Some(path) = path.as_str()
            && let Some((name, digest)) = media_name(path, kind)?
        {
            refs.insert((name, digest, kind.limit));
        }
        Ok(())
    };
    for pair in report["comparisons"]
        .as_array()
        .ok_or("quality.json has no comparisons list")?
    {
        add(&pair["file"], &JSON)?;
        add(&pair["baseline"]["wav"], &WAV)?;
        add(&pair["candidate"]["wav"], &WAV)?;
        for plot in pair["plots"].as_array().into_iter().flatten() {
            add(plot, &PNG)?;
        }
    }
    if refs.len() > MAX_MEDIA_FILES {
        return Err("quality.json references too many portable media files".into());
    }
    Ok(refs)
}

fn require_directory(dir: &Path) -> Result<(), String> {
    if !fs::symlink_metadata(dir).map_err(problem)?.is_dir()
        || dir.canonicalize().map_err(problem)? != dir
    {
        return Err(format!("{} must be a regular directory", dir.display()));
    }
    Ok(())
}

fn read_bounded(file: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let read = || -> Result<Vec<u8>, String> {
        let metadata = fs::symlink_metadata(file).map_err(problem)?;
        if !metadata.is_file() || file.canonicalize().map_err(problem)? != file {
            return Err("expected a regular file inside the report folder".into());
        }
        if metadata.len() > limit {
            return Err("file exceeds the report size limit".into());
        }
        let mut reader = File::open(file).map_err(problem)?;
        if !reader.metadata().map_err(problem)?.is_file() {
            return Err("expected a regular file inside the report folder".into());
        }
        let mut bytes = Vec::new();
        reader
            .by_ref()
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(problem)?;
        if bytes.len() as u64 > limit {
            return Err("file exceeds the report size limit".into());
        }
        Ok(bytes)
    };
    read().map_err(|error| format!("{}: {error}", file.display()))
}

fn collect_report(
    source: &Path,
    module: &str,
    expected: &str,
    checksum_error: &str,
    mut collect: impl FnMut(&Path, &[u8]) -> Result<(), String>,
) -> Result<(), String> {
    require_directory(source)?;
    let report = read_bounded(&source.join(REPORT), JSON.limit)?;
    if sha256_hex(&report) != expected {
        return Err(checksum_error.into());
    }
    let parsed = parse_report(&report, module)?;
    let references = media_refs(&parsed)?;
    collect(Path::new(REPORT), &report)?;
    let media = source.join(MEDIA);
    if !references.is_empty() {
        require_directory(&media)?;
    }
    let mut remaining = MAX_BUNDLE - report.len() as u64;
    for (name, expected, limit) in references {
        let bytes = read_bounded(&media.join(name), limit.min(remaining))?;
        if sha256_hex(&bytes) != expected {
            return Err(format!("portable report file failed its checksum: {name}"));
        }
        remaining -= bytes.len() as u64;
        collect(&Path::new(MEDIA).join(name), &bytes)?;
    }
    Ok(())
}

fn copy_report(root: &Path, source: &ReportSource, module: &str) -> Result<String, String> {
    let folder = source.folder.canonicalize().map_err(problem)?;
    let digest = &source.sha256;
    let dir = root.join("dsp");
    match fs::create_dir(&dir) {
        Ok(()) => (),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(error) => return Err(problem(error)),
    }
    require_directory(&dir)?;
    let stage = tempfile::tempdir_in(&dir).map_err(problem)?;
    collect_report(
        &folder,
        module,
        digest,
        "quality.json changed since it was opened. Reopen the report folder before saving.",
        |relative, bytes| {
            let path = stage.path().join(relative);
            let parent = path.parent().expect("staged report file has a parent");
            fs::create_dir_all(parent).map_err(problem)?;
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .expect("validated report file name");
            atomic(parent, name, bytes)
        },
    )?;
    let folder = dir.join(digest);
    if folder.exists() {
        collect_report(
            &folder,
            module,
            digest,
            "existing session report failed its checksum",
            |_, _| Ok(()),
        )?;
    } else {
        fs::rename(stage.path(), &folder).map_err(problem)?;
        #[cfg(unix)]
        File::open(&dir)
            .and_then(|file| file.sync_all())
            .map_err(problem)?;
    }
    Ok(format!("dsp/{digest}"))
}

pub(super) fn save(
    root: &Path,
    module: String,
    source: Option<ReportSource>,
) -> Result<OpenSession, String> {
    if !valid_module_id(&module) {
        return Err("Choose a lowercase module ID in author/name form.".into());
    }
    let mut session = read(root)?;
    let report_folder = match source {
        Some(source) => Some(copy_report(&session.root, &source, &module)?),
        None => session
            .document
            .dsp
            .take()
            .filter(|dsp| dsp.module == module)
            .and_then(|dsp| dsp.report_folder),
    };
    session.document.dsp = Some(Dsp {
        module,
        report_folder,
    });
    session.document.publish(&session.root)?;
    Ok(session)
}
