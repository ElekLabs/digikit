//! `patchkit syspkg pack BODY --out FILE --table MAJOR.MINOR`: the system package, the card file the resident stage 1 loads
//! (docs/plans/resident-core-and-system-package.md). The file is the 48-byte header in front of the body. The header is `dkrt_sdk::syspkg::Header`, the
//! type the runtime reads it with: patchkit only adds the body's sha256. `--table` is the resident table version the body was built against.

use crate::error::*;
use dkrt_sdk::syspkg::Header;
use sha2::{Digest, Sha256};

/// The package file of `body` built against resident table `table_major.table_minor`.
pub fn pack(body: &[u8], table_major: u16, table_minor: u16) -> Result<Vec<u8>> {
    let size = u32::try_from(body.len())
        .map_err(|_| err!("system package body of {} bytes is over 4 GiB", body.len()))?;
    let sha256: [u8; 32] = Sha256::digest(body).into();
    let mut file = Header::new(table_major, table_minor, size, sha256)
        .encode()
        .to_vec();
    file.extend_from_slice(body);
    Ok(file)
}

fn table(s: &str) -> Result<(u16, u16)> {
    let (major, minor) = s
        .split_once('.')
        .ok_or_else(|| err!("--table wants MAJOR.MINOR, got {s}"))?;
    let num = |t: &str| {
        t.parse::<u16>()
            .map_err(|_| err!("--table wants MAJOR.MINOR, got {s}"))
    };
    Ok((num(major)?, num(minor)?))
}

const USAGE: &str = "usage: patchkit syspkg pack BODY --out FILE --table MAJOR.MINOR";

pub fn run(args: &[String]) -> Result<()> {
    let (mut body, mut out, mut tab) = (None, None, None);
    let mut it = args.iter();
    if it.next().map(String::as_str) != Some("pack") {
        return Err(err!("{USAGE}"));
    }
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" => out = it.next(),
            "--table" => tab = it.next(),
            _ if body.is_none() => body = Some(a),
            _ => return Err(err!("{USAGE}")),
        }
    }
    let (Some(body), Some(out), Some(tab)) = (body, out, tab) else {
        return Err(err!("{USAGE}"));
    };
    let (major, minor) = table(tab)?;
    let bytes = std::fs::read(body).map_err(|e| err!("{body}: {e}"))?;
    let file = pack(&bytes, major, minor)?;
    if let Some(dir) = std::path::Path::new(out).parent() {
        std::fs::create_dir_all(dir).map_err(|e| err!("{}: {e}", dir.display()))?;
    }
    std::fs::write(out, &file).map_err(|e| err!("{out}: {e}"))?;
    println!("{out}: {} bytes, table {major}.{minor}", file.len());
    Ok(())
}
