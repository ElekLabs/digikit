//! The temporary include directory that holds the generated `workbench_descriptor.h` during a build, and the digest that keys the
//! object cache on it.

use super::c_header::CHeader;
use super::schema::Schema;
use crate::build::toolchain::TempDir;
use crate::error::*;
use sha2::{Digest, Sha256};
use std::path::Path;

const HEADER: &str = "workbench_descriptor.h";

pub(in crate::build) struct Header {
    include: TempDir,
    digest: [u8; 32],
}

impl Header {
    pub(in crate::build) fn read(
        moddir: &Path,
        id: &str,
        name: Option<&str>,
    ) -> Result<Option<Self>> {
        let Some(schema) = Schema::read(moddir, id, name)? else {
            return Ok(None);
        };
        let header = CHeader::render(&schema);

        let include = TempDir::new("pk-workbench-")?;
        let path = include.path().join(HEADER);
        std::fs::write(&path, &header).map_err(|e| err!("{}: {e}", path.display()))?;
        // The temporary include location varies between calls. Hash the declaration and generated content only.
        let mut digest = Sha256::new();
        digest.update(schema.source().as_bytes());
        digest.update(b"\0");
        digest.update(header.as_bytes());
        Ok(Some(Self {
            include,
            digest: digest.finalize().into(),
        }))
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
