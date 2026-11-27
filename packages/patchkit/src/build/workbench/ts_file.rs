//! The generated `workbench/gen/settings.ts` of one module: where it lives, and whether the file on disk is current.

use super::schema::Schema;
use super::ts_schema::TsSchema;
use crate::error::*;
use std::path::{Path, PathBuf};

pub struct TsFile {
    path: PathBuf,
    text: String,
}

impl TsFile {
    /// The file for the module in `moddir`; `None` when it has no `workbench.toml`.
    pub(crate) fn read(moddir: &Path, id: &str, name: Option<&str>) -> Result<Option<TsFile>> {
        if !moddir.join("workbench.toml").exists() {
            return Ok(None);
        }
        let Some(schema) = Schema::read(moddir, id, name)? else {
            return Ok(None);
        };
        Ok(Some(TsFile {
            path: moddir.join("workbench/gen/settings.ts"),
            text: TsSchema::render(&schema),
        }))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The generated text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// True when the file exists and holds the generated text.
    pub fn is_current(&self) -> bool {
        std::fs::read_to_string(&self.path).is_ok_and(|on_disk| on_disk == self.text)
    }

    pub fn write(&self) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| err!("{}: {e}", dir.display()))?;
        }
        std::fs::write(&self.path, &self.text).map_err(|e| err!("{}: {e}", self.path.display()))
    }
}

#[cfg(test)]
mod tests;
