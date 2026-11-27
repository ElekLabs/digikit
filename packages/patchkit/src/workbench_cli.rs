//! `patchkit workbench-ts MODDIR [--write|--check]`: the generated TypeScript settings file of a module (the library is
//! `patchkit::build::workbench_ts`).

use patchkit::build::workbench_ts;
use patchkit::error::{Error, Result};
use std::path::Path;

const USAGE: &str = "usage: patchkit workbench-ts MODDIR [--write|--check]";

pub fn run(rest: &[String]) -> Result<()> {
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
    let Some(file) = workbench_ts(Path::new(dir))? else {
        return Ok(());
    };
    if write {
        file.write()
    } else if check {
        if file.is_current() {
            Ok(())
        } else {
            Err(Error(format!(
                "{} is missing or out of date: run patchkit workbench-ts {dir} --write",
                file.path().display()
            )))
        }
    } else {
        print!("{}", file.text());
        Ok(())
    }
}
