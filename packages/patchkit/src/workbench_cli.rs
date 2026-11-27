//! `patchkit workbench-ts MODDIR [--write|--check]`: the generated TypeScript of a module (the library is `patchkit::build::workbench_ts`):
//! the settings file from `workbench.toml`, and from `workbench/*.proto` the protobuf-es files and the methods list (`build::wire::WireFiles`).

use patchkit::build::wire::WireFiles;
use patchkit::build::workbench_ts;
use patchkit::error::{Error, Result};
use std::path::Path;

const USAGE: &str = "usage: patchkit workbench-ts MODDIR [--write|--check]";

pub fn run(root: &Path, rest: &[String]) -> Result<()> {
    let mut dirs = vec![];
    let (mut write, mut check) = (false, false);
    for arg in rest {
        match arg.as_str() {
            "--write" => write = true,
            "--check" => check = true,
            flag if flag.starts_with("--") => return Err(Error(USAGE.into())),
            dir => dirs.push(dir),
        }
    }
    let [dir] = dirs[..] else {
        return Err(Error(USAGE.into()));
    };
    if write && check {
        return Err(Error(USAGE.into()));
    }
    let dir = Path::new(dir);
    let settings = workbench_ts(dir)?;
    let wire = WireFiles::read(root, dir)?;
    if write {
        // the wire files first: they refuse a removed or reordered method before anything is written
        if let Some(wire) = &wire {
            wire.write()?;
        }
        settings.map_or(Ok(()), |file| file.write())
    } else if check {
        let mut stale: Vec<String> = vec![];
        if let Some(file) = settings.as_ref().filter(|f| !f.is_current()) {
            stale.push(file.path().display().to_string());
        }
        if let Some(wire) = &wire {
            stale.extend(wire.stale().iter().map(|p| p.display().to_string()));
        }
        if stale.is_empty() {
            Ok(())
        } else {
            Err(Error(format!(
                "{} missing or out of date: run patchkit workbench-ts {} --write",
                stale.join(", "),
                dir.display()
            )))
        }
    } else {
        if let Some(file) = settings {
            print!("{}", file.text());
        }
        if let Some(wire) = &wire {
            for path in wire.paths() {
                println!("{}", path.display());
            }
        }
        Ok(())
    }
}
