//! The temporary include directory that holds the generated `workbench_descriptor.h` (the settings) and the generated query files
//! (`workbench_queries.h` and `.c`) during a build, and the digest that keys the object cache on them.

use super::c_header::CHeader;
use super::schema::Schema;
use crate::build::toolchain::TempDir;
use crate::build::wire::{self, Queries};
use crate::error::*;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const HEADER: &str = "workbench_descriptor.h";

pub(in crate::build) struct Header {
    include: TempDir,
    digest: [u8; 32],
    queries: Option<PathBuf>,
}

impl Header {
    /// The generated files of the module in `moddir`: its settings (`workbench.toml`) and its queries (`workbench/*.proto`); `None` when it
    /// has neither. `root` is the repository.
    pub(in crate::build) fn read(
        root: &Path,
        moddir: &Path,
        id: &str,
        name: Option<&str>,
    ) -> Result<Option<Self>> {
        let schema = Schema::read(moddir, id, name)?;
        let queries = Queries::read(root, moddir)?;
        if schema.is_none() && queries.is_none() {
            return Ok(None);
        }
        let include = TempDir::new("pk-workbench-")?;
        // The temporary include location varies between calls. Hash the declaration and generated content only.
        let mut digest = Sha256::new();
        if let Some(schema) = &schema {
            let header = CHeader::render(schema);
            let path = include.path().join(HEADER);
            std::fs::write(&path, &header).map_err(|e| err!("{}: {e}", path.display()))?;
            digest.update(schema.source().as_bytes());
            digest.update(b"\0");
            digest.update(header.as_bytes());
        }
        let mut source = None;
        if let Some(queries) = &queries {
            for (file, text) in [
                (wire::HEADER, queries.header()),
                (wire::SOURCE, queries.source()),
            ] {
                let path = include.path().join(file);
                std::fs::write(&path, text).map_err(|e| err!("{}: {e}", path.display()))?;
            }
            source = Some(include.path().join(wire::SOURCE));
            digest.update(b"\0queries");
            digest.update(queries.digest());
        }
        Ok(Some(Self {
            include,
            digest: digest.finalize().into(),
            queries: source,
        }))
    }

    /// The generated query source, which is compiled with the module.
    pub(in crate::build) fn queries_source(&self) -> Option<&Path> {
        self.queries.as_deref()
    }

    pub(in crate::build) fn dir(&self) -> &Path {
        self.include.path()
    }

    pub(in crate::build) fn digest(&self) -> &[u8; 32] {
        &self.digest
    }
}

#[cfg(test)]
mod tests;
