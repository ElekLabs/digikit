//! `patchkit wire`: the C codec and the descriptor set of a module query schema (`.proto`, see `build::wire`).
//!
//! - `patchkit wire c --proto FILE --include DIR... --prefix NAME [--out FILE]`: the C header (stdout without `--out`).
//! - `patchkit wire descriptor --proto FILE --include DIR... --out FILE`: the binary descriptor set, `dk` options kept (input of the
//!   TypeScript generator).
//!
//! `FILE` is relative to one of the `--include` directories; `dk/options.proto` must be reachable (sdk/proto).

use crate::build::wire::{CCodec, Schema};
use crate::error::*;
use std::path::{Path, PathBuf};

fn values(rest: &[String], name: &str) -> Vec<String> {
    rest.windows(2)
        .filter(|w| w[0] == name)
        .map(|w| w[1].clone())
        .collect()
}

pub fn run(rest: &[String]) -> Result<()> {
    let (Some(verb), args) = (rest.first(), rest.get(1..).unwrap_or_default()) else {
        return Err(err!(
            "usage: patchkit wire c|descriptor --proto FILE --include DIR... [--prefix NAME] [--out FILE]"
        ));
    };
    let one = |name: &str| values(args, name).pop();
    let proto = one("--proto").ok_or_else(|| err!("--proto FILE is required"))?;
    let dirs: Vec<PathBuf> = values(args, "--include")
        .into_iter()
        .map(PathBuf::from)
        .collect();
    let includes: Vec<&Path> = dirs.iter().map(PathBuf::as_path).collect();
    let bytes = match verb.as_str() {
        "c" => {
            let prefix = one("--prefix").ok_or_else(|| err!("--prefix NAME is required"))?;
            let schema = Schema::read(&includes, &proto)?;
            CCodec::render(&schema, &prefix).into_bytes()
        }
        "descriptor" => Schema::descriptor_set(&includes, &proto)?,
        other => return Err(err!("patchkit wire: unknown verb {other}")),
    };
    match one("--out") {
        Some(out) => std::fs::write(&out, bytes).map_err(|e| err!("{out}: {e}")),
        None => {
            use std::io::Write as _;
            std::io::stdout()
                .write_all(&bytes)
                .map_err(|e| err!("stdout: {e}"))
        }
    }
}
