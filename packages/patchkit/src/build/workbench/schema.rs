//! The checked contents of a module's `workbench.toml`. The descriptor encoder and the C and TypeScript renderers take a `Schema`; none
//! of them reads TOML.

use super::declaration::Declaration;
use super::descriptor::MAX_STRING_BYTES;
use super::setting::Setting;
use crate::error::*;
use std::collections::HashSet;
use std::path::Path;

/// The most settings a descriptor holds: its count takes one byte.
const MAX_SETTINGS: usize = 255;

#[derive(Debug)]
pub(super) struct Schema {
    module: String,
    name: String,
    settings: Vec<Setting>,
    descriptor: Vec<u8>,
    source: String,
}

impl Schema {
    /// Reads and checks `source` for the module `module` that shows `name` on the device. Fails with the first broken rule, also when
    /// the descriptor would not fit.
    pub(super) fn parse(source: &str, module: &str, name: &str) -> Result<Schema> {
        let doc: Declaration = toml::from_str(source).map_err(|e| err!("{e}"))?;
        if doc.workbench.version != 1 {
            return Err(err!("unsupported Workbench descriptor version"));
        }
        if !(1..=MAX_STRING_BYTES).contains(&name.len()) {
            return Err(err!(
                "module name must be 1 to {MAX_STRING_BYTES} UTF-8 bytes"
            ));
        }
        if !(1..=MAX_SETTINGS).contains(&doc.setting.len()) {
            return Err(err!("expected 1 to {MAX_SETTINGS} public settings"));
        }
        let mut ids = HashSet::new();
        let mut symbols = HashSet::new();
        let mut settings = vec![];
        for raw in &doc.setting {
            if !ids.insert(raw.id.as_str()) {
                return Err(err!("duplicate Workbench ID {:?}", raw.id));
            }
            let setting = Setting::parse(raw, module)?;
            for symbol in setting.symbols() {
                if !symbols.insert(symbol.clone()) {
                    return Err(err!(
                        "setting {:?}: the C name {symbol} is used twice",
                        raw.id
                    ));
                }
            }
            settings.push(setting);
        }
        for operation in &doc.operation {
            if operation.id.is_empty() {
                return Err(err!("Workbench operation ID must not be empty"));
            }
            if !ids.insert(operation.id.as_str()) {
                return Err(err!("duplicate Workbench ID {:?}", operation.id));
            }
        }
        let mut schema = Schema {
            module: module.into(),
            name: name.into(),
            settings,
            descriptor: vec![],
            source: source.into(),
        };
        schema.descriptor = schema.encode()?;
        Ok(schema)
    }

    /// The schema of the `workbench.toml` in `moddir`; `None` when the module has none. Errors name the file.
    pub(super) fn read(moddir: &Path, module: &str, name: Option<&str>) -> Result<Option<Schema>> {
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
        let schema =
            Schema::parse(&source, module, name).map_err(|e| err!("{}: {e}", path.display()))?;
        Ok(Some(schema))
    }

    /// The text of `workbench.toml` this schema was read from.
    pub(super) fn source(&self) -> &str {
        &self.source
    }

    /// The binary descriptor the module registers.
    pub(super) fn descriptor(&self) -> &[u8] {
        &self.descriptor
    }

    pub(super) fn module(&self) -> &str {
        &self.module
    }

    pub(super) fn name(&self) -> &str {
        &self.name
    }

    pub(super) fn settings(&self) -> &[Setting] {
        &self.settings
    }

    #[cfg(test)]
    pub(super) fn setting(&self, id: &str) -> Option<&Setting> {
        self.settings.iter().find(|s| s.id() == id)
    }

    /// The least runtime the module needs for its descriptor: 0 for version 1, else the runtime that first accepts version 2.
    pub(super) fn runtime_floor(&self) -> u32 {
        match self.descriptor_version() {
            1 => 0,
            _ => crate::sdkgen::workbench_typed_runtime(),
        }
    }

    /// The first byte of the binary descriptor: 1 while every setting is text or a file name, else 2.
    pub(super) fn descriptor_version(&self) -> u8 {
        if self.settings.iter().all(|s| s.kind().is_version_1()) {
            1
        } else {
            2
        }
    }
}

#[cfg(test)]
mod tests;
