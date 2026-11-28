//! Portable bundle IO. No dialogs, window lifecycle or UI state.
use super::{
    document::{Attachment, BASE, Document, Firmware, asset, atomic, problem},
    sparse,
};
use digi_control::{
    cards::{PRIVATE_CARD_MARKER, build_card},
    firmware::sha256_hex,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

#[cfg(test)]
mod tests;

#[derive(Serialize)]
pub(crate) struct OpenSession {
    pub(super) root: PathBuf,
    pub(super) document: Document,
}

pub(super) fn read(root: &Path) -> Result<OpenSession, String> {
    let root = root.canonicalize().map_err(problem)?;
    let document = Document::open(&root)?;
    Ok(OpenSession { root, document })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Create {
    destination: PathBuf,
    name: String,
    firmware: PathBuf,
    source: String,
    card: Option<PathBuf>,
    fork: Option<PathBuf>,
    runtime: bool,
    dsp: bool,
}

fn identify(path: PathBuf, runtime: bool, dsp: bool) -> Result<(Firmware, Vec<u8>), String> {
    let path = path.canonicalize().map_err(problem)?;
    let bytes = fs::read(&path).map_err(problem)?;
    let device = digi_control::runtime_patch::device_short(&bytes)
        .ok_or("not a supported firmware image")?;
    let registry = device_profile::Registry::load().map_err(problem)?;
    let profile = registry
        .devices()
        .iter()
        .find(|p| p.short == device)
        .ok_or("unknown firmware device")?;
    if profile.card.is_none() {
        return Err("this firmware has no emulated card support".into());
    }
    Ok((
        Firmware {
            path,
            sha256: sha256_hex(&bytes),
            device: device.into(),
            runtime,
            dsp,
            image_sha256: None,
        },
        bytes,
    ))
}

pub(super) trait WorkingCard {
    fn path(&self) -> Result<PathBuf, String>;
    fn read(&self, card: &Path) -> Result<CardCheckpoint, String>;
}

pub(super) struct CardCheckpoint {
    bytes: Vec<u8>,
    image: String,
    base: String,
    dsp: bool,
}

impl CardCheckpoint {
    pub(super) fn new(bytes: Vec<u8>, image: String, base: String, dsp: bool) -> Self {
        Self {
            bytes,
            image,
            base,
            dsp,
        }
    }

    pub(super) fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    fn pin(self, fw: &mut Firmware) -> Result<Vec<u8>, String> {
        if fw.sha256 != self.image && fw.sha256 != self.base {
            return Err("the running firmware does not match the session firmware".into());
        }
        if fw
            .image_sha256
            .as_deref()
            .is_some_and(|expected| expected != self.image)
        {
            return Err("the running patch image has changed".into());
        }
        fw.runtime = self.image != fw.sha256;
        fw.image_sha256 = Some(self.image);
        fw.dsp = self.dsp;
        Ok(self.bytes)
    }
}

pub(super) fn write_overlay(root: &Path, doc: &mut Document, bytes: &[u8]) -> Result<(), String> {
    let sha = sha256_hex(bytes);
    let dir = root.join("card");
    if dir.canonicalize().map_err(problem)? != dir {
        return Err("session card folder must not be a symlink".into());
    }
    atomic(&dir, &format!("{sha}.overlay"), bytes)?;
    doc.checkpoint = Some(sha);
    Ok(())
}

pub(super) fn create(working: &impl WorkingCard, input: Create) -> Result<OpenSession, String> {
    let destination = input.destination;
    let parent = destination
        .parent()
        .ok_or("pick a parent folder")?
        .canonicalize()
        .map_err(problem)?;
    let leaf = destination
        .file_name()
        .ok_or("pick a new session folder name")?;
    let root = parent.join(leaf);
    if root.exists() {
        return Err("choose a new folder name; Save As does not overwrite sessions".into());
    }
    let staging = tempfile::Builder::new()
        .prefix(".digikit-session-")
        .tempdir_in(&parent)
        .map_err(problem)?;
    let stage = staging.path();
    let base = stage.join(BASE);
    fs::create_dir(stage.join("card")).map_err(problem)?;
    let previous = input.fork.as_deref().map(read).transpose()?;
    let (firmware, firmware_bytes) = if let Some(previous) = &previous {
        (previous.document.firmware.clone(), Vec::new())
    } else {
        identify(input.firmware, input.runtime, input.dsp)?
    };
    let id = stage
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let name = input.name.trim();
    let mut doc = Document::new(
        id,
        if name.is_empty() {
            "Untitled Session".into()
        } else {
            name.into()
        },
        firmware,
    );
    let mut overlay = None;
    if let Some(previous) = &previous {
        doc.dsp = previous.document.dsp.as_ref().map(|dsp| dsp.selection());
        sparse::copy(&asset(&previous.root, BASE)?, &base).map_err(problem)?;
        overlay = if let Some(card) = &input.card {
            Some(working.read(card)?.pin(&mut doc.firmware)?)
        } else {
            previous
                .document
                .overlay(&previous.root)?
                .map(fs::read)
                .transpose()
                .map_err(problem)?
        };
    } else if input.source == "folder" {
        let dir = input
            .card
            .as_deref()
            .filter(|p| p.is_dir())
            .ok_or("choose a +Drive folder")?;
        build_card(&doc.firmware.device, &firmware_bytes, dir, &base, |_, _| {})?;
    } else {
        let card = match input.source.as_str() {
            "live" => working.path()?,
            "image" => input.card.ok_or("choose a card image")?,
            _ => return Err("unknown card source".into()),
        };
        let meta = fs::metadata(&card).map_err(problem)?;
        if !meta.is_file() || meta.len() == 0 || meta.len() % 512 != 0 {
            return Err("card image must be a nonempty, 512-byte-aligned file".into());
        }
        sparse::copy(&card, &base).map_err(problem)?;
        if input.source == "live" {
            overlay = Some(working.read(&card)?.pin(&mut doc.firmware)?);
        }
    }
    if let Some(bytes) = overlay {
        write_overlay(stage, &mut doc, &bytes)?;
    }
    doc.publish(stage)?;
    fs::rename(stage, &root).map_err(problem)?;
    Ok(OpenSession {
        root,
        document: doc,
    })
}

pub(super) fn save(
    working: &impl WorkingCard,
    root: &Path,
    card: Option<&Path>,
    name: &str,
) -> Result<OpenSession, String> {
    let mut session = read(root)?;
    let doc = &mut session.document;
    if let Some(card) = card {
        let bytes = working.read(card)?.pin(&mut doc.firmware)?;
        write_overlay(&session.root, doc, &bytes)?;
    }
    if !name.trim().is_empty() {
        doc.name = name.trim().into();
    }
    doc.publish(&session.root)?;
    Ok(session)
}

pub(super) fn mount(session: &OpenSession) -> Result<(TempDir, PathBuf), String> {
    let base = asset(&session.root, BASE)?;
    let overlay = session.document.overlay(&session.root)?;
    let dir = tempfile::Builder::new()
        .prefix("digikit-mount-")
        .tempdir()
        .map_err(problem)?;
    let image = dir.path().join("card.img");
    if fs::hard_link(&base, &image).is_err() {
        sparse::copy(&base, &image).map_err(problem)?;
    }
    let stamp = fs::metadata(&base)
        .and_then(|m| m.modified())
        .map_err(problem)?;
    let key = format!("workbench:{}:{base:?}:{stamp:?}", session.document.id);
    fs::write(dir.path().join("card.key"), key).map_err(problem)?;
    fs::write(
        dir.path().join(PRIVATE_CARD_MARKER),
        if overlay.is_some() { "used" } else { "fresh" },
    )
    .map_err(problem)?;
    if let Some(src) = overlay {
        fs::copy(src, dir.path().join("overlay.bin")).map_err(problem)?;
    }
    Ok((dir, image))
}

pub(super) fn attach(root: PathBuf, source: PathBuf) -> Result<OpenSession, String> {
    let mut session = read(&root)?;
    let name = source
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("invalid file name")?
        .to_string();
    let bytes = fs::read(&source).map_err(problem)?;
    let (kind, ext) = if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
        ("audio", "wav")
    } else {
        let value: Value = serde_json::from_slice(&bytes).map_err(problem)?;
        match value["schema"].as_str() {
            Some("dk.profile/1" | "dk.calcaptures/1") => ("capture", "capture.json"),
            Some("dk.audio-render/1" | "dk.audio-compare/1") => ("comparison", "json"),
            _ => return Err("not a supported capture, WAV or audio report".into()),
        }
    };
    let leaf = format!("{}.{ext}", sha256_hex(&bytes));
    let dir = session.root.join("attachments");
    fs::create_dir_all(&dir).map_err(problem)?;
    if dir.canonicalize().map_err(problem)? != dir {
        return Err("session attachments folder must not be a symlink".into());
    }
    atomic(&dir, &leaf, &bytes)?;
    let path = format!("attachments/{leaf}");
    if !session.document.attachments.iter().any(|a| a.path == path) {
        session.document.attachments.push(Attachment {
            name,
            kind: kind.into(),
            path,
        });
        session.document.publish(&session.root)?;
    }
    Ok(session)
}
