//! The queries of one module: its `workbench/<name>.proto` (one file, one `service`), checked, with the C that patchkit generates from it
//! and the digest that keys the module build on it.

use super::budget;
use super::c_codec::CCodec;
use super::schema::Schema;
use crate::error::*;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// The generated files' names: the module includes the header, patchkit compiles the source with the module.
pub const HEADER: &str = "workbench_queries.h";
pub const SOURCE: &str = "workbench_queries.c";

pub struct Queries {
    proto: PathBuf,
    /// The proto's file name and the directories its imports resolve in.
    file: String,
    includes: Vec<PathBuf>,
    name: String,
    schema: Schema,
    header: String,
    source: String,
    digest: [u8; 32],
}

impl Queries {
    /// The queries of the module in `moddir`; `None` when it has no `workbench/*.proto`. `root` is the repository (`sdk/proto` holds
    /// `dk/options.proto`).
    pub fn read(root: &Path, moddir: &Path) -> Result<Option<Queries>> {
        let dir = moddir.join("workbench");
        let mut protos: Vec<PathBuf> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "proto"))
            .collect();
        protos.sort();
        let proto = match protos.as_slice() {
            [] => return Ok(None),
            [one] => one.clone(),
            [_, second, ..] => {
                return Err(err!(
                    "{}: a module has one .proto, found {} as well",
                    dir.display(),
                    second.file_name().unwrap_or_default().to_string_lossy()
                ));
            }
        };
        let file = proto
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let name = file.trim_end_matches(".proto").to_string();
        let options = root.join("sdk/proto");
        let schema = Schema::read(&[&dir, &options], &file)?;
        let Some(service) = &schema.service else {
            return Err(err!("{}: the schema needs one service", proto.display()));
        };
        if service.methods.is_empty() {
            return Err(err!("{}: the service has no method", proto.display()));
        }
        if schema.package.is_empty() {
            return Err(err!(
                "{}: the schema needs a package (it starts the C names)",
                proto.display()
            ));
        }
        budget::check(&schema).map_err(|e| err!("{}: {e}", proto.display()))?;
        let prefix = schema.package.replace('.', "_");
        let generated = CCodec::render(&schema, &prefix, HEADER);
        let mut digest = Sha256::new();
        for part in [
            read(&proto)?.as_slice(),
            read(&options.join("dk/options.proto"))?.as_slice(),
            generated.header.as_bytes(),
            generated.source.as_bytes(),
        ] {
            digest.update(part);
            digest.update(b"\0");
        }
        Ok(Some(Queries {
            proto,
            file,
            includes: vec![dir, options],
            name,
            schema,
            header: generated.header,
            source: generated.source,
            digest: digest.finalize().into(),
        }))
    }

    /// The proto file's name without the extension: the name of the generated TypeScript.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn proto(&self) -> &Path {
        &self.proto
    }

    pub fn header(&self) -> &str {
        &self.header
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// The proto, the options and the generated C: the part of the build key that the queries add.
    pub fn digest(&self) -> &[u8; 32] {
        &self.digest
    }

    /// The descriptor set of the schema and its imports (input of the protobuf-es generator).
    pub fn descriptor_set(&self) -> Result<Vec<u8>> {
        let includes: Vec<&Path> = self.includes.iter().map(PathBuf::as_path).collect();
        Schema::descriptor_set(&includes, &self.file)
    }

    /// The method names in service order: the method number is the position plus one.
    pub fn methods(&self) -> Vec<String> {
        self.schema
            .service
            .iter()
            .flat_map(|s| s.methods.iter().map(|m| m.name.clone()))
            .collect()
    }
}

fn read(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|e| err!("{}: {e}", path.display()))
}
