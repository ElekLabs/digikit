//! The public settings descriptor compiled into a module from `workbench.toml`.

use super::toolchain::TempDir;
use crate::error::*;
use dkrt_sdk::settings::{KEY_MAX, module_key};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::Path;

const HEADER: &str = "workbench_descriptor.h";
const MAX_BYTES: usize = 512;
const MAX_TEXT_BYTES: u8 = 64;
const MAX_FILE_NAME_BYTES: u8 = 47;
const MAX_EXTENSION_BYTES: usize = 16;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Declaration {
    workbench: Version,
    #[serde(default)]
    setting: Vec<Field>,
    #[serde(default)]
    operation: Vec<PreviewOperation>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Version {
    version: u8,
    #[serde(rename = "component")]
    _component: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Field {
    id: String,
    label: String,
    #[serde(rename = "type")]
    kind: FieldType,
    max_bytes: u8,
    #[serde(default)]
    apply: Apply,
    extension: Option<String>,
    reset_label: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum FieldType {
    Text,
    FileName,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Apply {
    #[default]
    Immediate,
    NextRestart,
}

// The preview companion uses these declarations. Device v1 carries public settings only.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewOperation {
    id: String,
    #[serde(rename = "output")]
    _output: PreviewOutput,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum PreviewOutput {
    OptionalText,
}

fn push_string(out: &mut Vec<u8>, what: &str, value: &str) -> Result<()> {
    let len = u8::try_from(value.len())
        .ok()
        .filter(|len| *len != 0)
        .ok_or_else(|| err!("{what} must be 1 to 255 UTF-8 bytes"))?;
    out.push(len);
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn encode(source: &str, id: &str, name: &str) -> Result<Vec<u8>> {
    let doc: Declaration = toml::from_str(source).map_err(|e| err!("{e}"))?;
    if doc.workbench.version != 1 {
        return Err(err!("unsupported Workbench descriptor version"));
    }
    let count = u8::try_from(doc.setting.len())
        .ok()
        .filter(|count| *count != 0)
        .ok_or_else(|| err!("expected 1 to 255 public settings"))?;
    let mut out = vec![1];
    push_string(&mut out, "module name", name)?;
    out.push(count);
    let mut ids = HashSet::new();
    for field in &doc.setting {
        if !ids.insert(field.id.as_str()) {
            return Err(err!("duplicate Workbench ID {:?}", field.id));
        }
        if module_key(id.as_bytes(), field.id.as_bytes(), &mut [0; KEY_MAX]).is_none() {
            return Err(err!("invalid module settings key {:?}", field.id));
        }
        push_string(&mut out, "settings key", &field.id)?;
        push_string(&mut out, "settings label", &field.label)?;
        let (kind, max_bytes) = match field.kind {
            FieldType::Text => (0, MAX_TEXT_BYTES),
            FieldType::FileName => (1, MAX_FILE_NAME_BYTES),
        };
        if !(1..=max_bytes).contains(&field.max_bytes) {
            return Err(err!(
                "setting {:?}: max_bytes must be 1 to {max_bytes}",
                field.id
            ));
        }
        let apply = match field.apply {
            Apply::Immediate => 0,
            Apply::NextRestart => 1,
        };
        out.extend_from_slice(&[field.max_bytes, kind, apply]);
        match (&field.kind, field.extension.as_deref()) {
            (FieldType::Text, None) if field.reset_label.is_none() => {}
            (FieldType::Text, _) => {
                return Err(err!(
                    "setting {:?}: extension and reset_label require type file-name",
                    field.id
                ));
            }
            (FieldType::FileName, extension) => {
                let extension = extension.unwrap_or_default();
                if !(1..=MAX_EXTENSION_BYTES).contains(&extension.len())
                    || !extension
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                {
                    return Err(err!(
                        "setting {:?}: extension must be 1 to {MAX_EXTENSION_BYTES} lowercase ASCII letters or digits",
                        field.id
                    ));
                }
                if usize::from(field.max_bytes) < extension.len() + 2 {
                    return Err(err!(
                        "setting {:?}: max_bytes must fit a stem, dot, and extension",
                        field.id
                    ));
                }
                push_string(&mut out, "file extension", extension)?;
                push_string(
                    &mut out,
                    "reset label",
                    field.reset_label.as_deref().unwrap_or("None"),
                )?;
            }
        }
        if out.len() > MAX_BYTES {
            return Err(err!("Workbench descriptor exceeds {MAX_BYTES} bytes"));
        }
    }
    for operation in &doc.operation {
        if operation.id.is_empty() {
            return Err(err!("Workbench operation ID must not be empty"));
        }
        if !ids.insert(operation.id.as_str()) {
            return Err(err!("duplicate Workbench ID {:?}", operation.id));
        }
    }
    Ok(out)
}

pub(super) struct Header {
    include: TempDir,
    digest: [u8; 32],
}

impl Header {
    pub(super) fn read(moddir: &Path, id: &str, name: Option<&str>) -> Result<Option<Self>> {
        let path = moddir.join("workbench.toml");
        let source = match std::fs::read_to_string(&path) {
            Ok(source) => source,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(err!("{}: {e}", path.display())),
        };
        let name = name.ok_or_else(|| {
            err!(
                "{}: missing [module].name",
                moddir.join("module.toml").display()
            )
        })?;
        let bytes = encode(&source, id, name).map_err(|e| err!("{}: {e}", path.display()))?;
        let mut header = String::from(
            "/* Generated by patchkit from module.toml and workbench.toml. */\n\
            #pragma once\n#include <stdint.h>\n\
            static const uint8_t dk_workbench_descriptor[] = {\n",
        );
        for row in bytes.chunks(12) {
            header.push_str("    ");
            for byte in row {
                write!(&mut header, "0x{byte:02x}, ").unwrap();
            }
            header.truncate(header.len() - 1);
            header.push('\n');
        }
        header.push_str("};\n");

        let include = TempDir::new("pk-workbench-")?;
        let path = include.path().join(HEADER);
        std::fs::write(&path, &header).map_err(|e| err!("{}: {e}", path.display()))?;
        // The temporary include location varies between calls. Hash the declaration and generated content only.
        let mut digest = Sha256::new();
        digest.update(source.as_bytes());
        digest.update(b"\0");
        digest.update(header.as_bytes());
        Ok(Some(Self {
            include,
            digest: digest.finalize().into(),
        }))
    }

    pub(super) fn dir(&self) -> &Path {
        self.include.path()
    }

    pub(super) fn digest(&self) -> &[u8; 32] {
        &self.digest
    }
}

#[cfg(test)]
mod tests;
