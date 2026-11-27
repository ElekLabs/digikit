use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};
use tempfile::NamedTempFile;

const SCHEMA: &str = "dk.session/1";
pub(super) const MANIFEST: &str = "session.json";
pub(super) const BASE: &str = "card/base.img";

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Firmware {
    pub(super) path: PathBuf,
    pub(super) sha256: String,
    pub(super) device: String,
    pub(super) runtime: bool,
    pub(super) dsp: bool,
    pub(super) image_sha256: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Attachment {
    pub(super) path: String,
    pub(super) kind: String,
    pub(super) name: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Document {
    schema: String,
    pub(super) id: String,
    pub(super) name: String,
    pub(super) firmware: Firmware,
    pub(super) checkpoint: Option<String>,
    #[serde(default)]
    pub(super) attachments: Vec<Attachment>,
}

impl Document {
    pub(super) fn new(id: String, name: String, firmware: Firmware) -> Self {
        Self {
            schema: SCHEMA.into(),
            id,
            name,
            firmware,
            checkpoint: None,
            attachments: Vec::new(),
        }
    }

    pub(super) fn open(root: &Path) -> Result<Self, String> {
        let file = asset(root, MANIFEST)?;
        let doc: Self =
            serde_json::from_reader(fs::File::open(&file).map_err(problem)?).map_err(problem)?;
        if doc.schema != SCHEMA
            || doc.id.is_empty()
            || !hash(&doc.firmware.sha256)
            || doc
                .firmware
                .image_sha256
                .as_deref()
                .is_some_and(|v| !hash(v))
            || doc.checkpoint.as_deref().is_some_and(|v| !hash(v))
        {
            return Err("not a supported Workbench session".into());
        }
        Ok(doc)
    }

    pub(super) fn publish(&self, root: &Path) -> Result<(), String> {
        let mut bytes = serde_json::to_vec_pretty(self).map_err(problem)?;
        bytes.push(b'\n');
        atomic(root, MANIFEST, &bytes)
    }

    pub(super) fn overlay(&self, root: &Path) -> Result<Option<PathBuf>, String> {
        self.checkpoint
            .as_ref()
            .map(|sha| {
                let file = asset(root, &format!("card/{sha}.overlay"))?;
                let bytes = fs::read(&file).map_err(problem)?;
                if digi_control::firmware::sha256_hex(&bytes) != *sha {
                    return Err("saved card checkpoint failed its checksum".into());
                }
                Ok(file)
            })
            .transpose()
    }
}

fn hash(v: &str) -> bool {
    v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit())
}

pub(super) fn asset(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let rel = Path::new(relative);
    if rel.as_os_str().is_empty() || !rel.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err("invalid session asset path".into());
    }
    let path = root.join(rel).canonicalize().map_err(problem)?;
    if !path.starts_with(root) || !path.is_file() {
        return Err(format!(
            "session asset is not a file inside the session: {relative}"
        ));
    }
    Ok(path)
}

pub(super) fn atomic(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    let mut tmp = NamedTempFile::new_in(dir).map_err(problem)?;
    tmp.write_all(bytes).map_err(problem)?;
    tmp.as_file().sync_all().map_err(problem)?;
    tmp.persist(dir.join(name)).map_err(problem)?;
    #[cfg(unix)]
    fs::File::open(dir)
        .and_then(|f| f.sync_all())
        .map_err(problem)?;
    Ok(())
}

pub(super) fn problem(error: impl std::fmt::Display) -> String {
    error.to_string()
}
