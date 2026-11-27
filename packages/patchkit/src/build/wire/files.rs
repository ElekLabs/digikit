//! The committed files a module's query schema generates beside its page (`workbench/gen/`): the protobuf-es code
//! (`<name>_pb.js`, `<name>_pb.d.ts`, made by packages/wire's generator from patchkit's descriptor set; no protoc or buf) and
//! `<name>.methods`, the method names in order. Methods are only appended: the number a call carries is the method's position.

use super::queries::Queries;
use crate::build::toolchain::TempDir;
use crate::error::*;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct WireFiles {
    /// (path, text) of each file.
    files: Vec<(PathBuf, String)>,
    methods_path: PathBuf,
    methods: Vec<String>,
}

impl WireFiles {
    /// The files of the module in `moddir`; `None` without `workbench/*.proto`. Runs `node` on packages/wire's generator.
    pub fn read(root: &Path, moddir: &Path) -> Result<Option<WireFiles>> {
        let Some(queries) = Queries::read(root, moddir)? else {
            return Ok(None);
        };
        let tmp = TempDir::new("pk-wire-ts-")?;
        let set = tmp.path().join("set.bin");
        std::fs::write(&set, queries.descriptor_set()?)
            .map_err(|e| err!("{}: {e}", set.display()))?;
        let out = tmp.path().join("out");
        let name = queries.name();
        let run = Command::new("node")
            .arg(root.join("packages/wire/scripts/module-gen.ts"))
            .arg("--descriptor")
            .arg(&set)
            .arg("--proto")
            .arg(format!("{name}.proto"))
            .arg("--out")
            .arg(&out)
            .output()
            .map_err(|e| err!("node: {e} (the protobuf-es files are made with node)"))?;
        if !run.status.success() {
            return Err(err!(
                "the protobuf-es generator failed: {}",
                String::from_utf8_lossy(&run.stderr)
            ));
        }
        let gen_dir = moddir.join("workbench/gen");
        let mut files = vec![];
        for file in [format!("{name}_pb.js"), format!("{name}_pb.d.ts")] {
            let text = std::fs::read_to_string(out.join(&file))
                .map_err(|e| err!("the generator wrote no {file}: {e}"))?;
            files.push((gen_dir.join(file), text));
        }
        let methods = queries.methods();
        let methods_path = gen_dir.join(format!("{name}.methods"));
        files.push((
            methods_path.clone(),
            methods.iter().map(|m| format!("{m}\n")).collect(),
        ));
        Ok(Some(WireFiles {
            files,
            methods_path,
            methods,
        }))
    }

    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        self.files.iter().map(|(p, _)| p.as_path())
    }

    /// The files that are missing or differ from the generated text.
    pub fn stale(&self) -> Vec<&Path> {
        self.files
            .iter()
            .filter(|(p, text)| std::fs::read_to_string(p).ok().as_ref() != Some(text))
            .map(|(p, _)| p.as_path())
            .collect()
    }

    /// Writes the files. Refuses when the methods file on disk names a method that is no longer at its position.
    pub fn write(&self) -> Result<()> {
        let before = std::fs::read_to_string(&self.methods_path).unwrap_or_default();
        for (i, old) in before.lines().enumerate() {
            if self.methods.get(i).map(String::as_str) != Some(old) {
                return Err(err!(
                    "{}: method {} ({old}) is {}: methods are only appended, the number a call carries is the position",
                    self.methods_path.display(),
                    i + 1,
                    self.methods
                        .get(i)
                        .map_or("removed".to_string(), |now| format!("now {now}")),
                ));
            }
        }
        for (path, text) in &self.files {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| err!("{}: {e}", dir.display()))?;
            }
            std::fs::write(path, text).map_err(|e| err!("{}: {e}", path.display()))?;
        }
        Ok(())
    }
}
