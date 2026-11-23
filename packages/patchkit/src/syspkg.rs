//! The system package, the card file the resident stage 1 loads (docs/plans/resident-core-and-system-package.md). The file is the 48-byte header in front of the body.
//! The header is `dkrt_sdk::syspkg::Header`, the type the runtime reads it with: patchkit only adds the body's sha256.
//!
//! - `patchkit syspkg pack BODY --out FILE --table MAJOR.MINOR`: any body (the stage-1 test fixture). `--table` is the resident table version the body was built against.
//! - `patchkit syspkg build OBJECT [--part OBJECT]... --out FILE`: the package artifact. `OBJECT` is the `[system_package]` object of `core/runtime` (`build/package.dko.json`); a
//!   `--part` is another object of the package (the DIGIKIT menu's C object, `[package_part]` of its module.toml) that is linked into it: the symbols it needs the package defines.
//!   The body is the linked image, its bss size and its relocations (`dkrt_sdk::syspkg::body`), and the table version is the one the package holds in `DKPKG_TABLE`.

use crate::dko::Dko;
use crate::error::*;
use crate::link;
use dkrt_sdk::syspkg::Header;
use dkrt_sdk::syspkg::body::{BodyHeader, HEADER_BYTES};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// The entry function the package exports.
pub const ENTRY: &str = "dkpkg_entry";
/// The word the package holds: the resident table version it was built against, `major << 16 | minor`.
pub const TABLE_SYMBOL: &str = "DKPKG_TABLE";
/// The word the package may hold for its capacity report: the bytes it takes in the package block (`BodyHeader::placed_bytes`), written here after the link. `UNSET` before.
pub const PLACED_SYMBOL: &str = "DKPKG_PLACED";
const PLACED_UNSET: u32 = 0xffff_ffff;

/// The body of the package object `obj` and the resident table version it was built against. The package is linked at address 0 with no import: a reference to a symbol
/// the object does not define would be a link by name, which the package does not have (it calls the resident core through its table).
pub fn body(obj: &Dko) -> Result<(Vec<u8>, u16, u16)> {
    if obj.arch != "cf" {
        return Err(err!(
            "the system package is ColdFire code; the object is {}",
            obj.arch
        ));
    }
    let defined: std::collections::BTreeSet<&str> = obj
        .symbols
        .iter()
        .filter(|s| s.section.is_some())
        .map(|s| s.name.as_str())
        .collect();
    let mut undefined: Vec<&str> = obj
        .relocs
        .iter()
        .map(|r| r.symbol.as_str())
        .chain(
            obj.symbols
                .iter()
                .filter(|s| s.section.is_none())
                .map(|s| s.name.as_str()),
        )
        .filter(|n| !defined.contains(n))
        .collect();
    undefined.sort();
    undefined.dedup();
    if !undefined.is_empty() {
        return Err(err!(
            "the system package links only through the resident table: unresolved symbol(s) {}",
            undefined.join(", ")
        ));
    }
    let mut linked = link::link(obj, 0, &BTreeMap::new())?;
    let kind_of: BTreeMap<&str, &str> = obj
        .sections
        .iter()
        .map(|s| (s.name.as_str(), s.kind.as_str()))
        .collect();
    let mut relocs: Vec<u32> = vec![];
    for r in &obj.relocs {
        if r.rtype != "cf.32" {
            // a PC-relative reference inside the package is final at link time
            continue;
        }
        if kind_of.get(r.section.as_str()) == Some(&"bss") {
            return Err(err!("relocation in bss section {}", r.section));
        }
        let at = linked.places[&r.section] + r.offset;
        if at + 4 > linked.image.len() as u64 {
            return Err(err!("relocation at {at:#x} is past the image"));
        }
        relocs.push(at as u32);
    }
    relocs.sort();
    let image_word = |name: &str| -> Result<u32> {
        let at = *linked
            .symbols
            .get(name)
            .ok_or_else(|| err!("the package defines no {name}"))? as usize;
        match linked.image.get(at..at + 4) {
            Some(w) if at < linked.image.len() => Ok(u32::from_be_bytes([w[0], w[1], w[2], w[3]])),
            _ => Err(err!("{name} is not a whole word of the image")),
        }
    };
    let entry = *linked
        .symbols
        .get(ENTRY)
        .ok_or_else(|| err!("the package defines no {ENTRY}"))?;
    let table = image_word(TABLE_SYMBOL)?;
    let align = obj
        .sections
        .iter()
        .filter(|s| s.kind != "bss" || s.size > 0)
        .map(|s| s.align)
        .max()
        .unwrap_or(4)
        .clamp(4, 16);
    let header = BodyHeader {
        entry: u32::try_from(entry).map_err(|_| err!("{ENTRY} is outside the image"))?,
        image: linked.image.len() as u32,
        bss: u32::try_from(linked.bss_size).map_err(|_| err!("bss over 4 GiB"))?,
        relocs: relocs.len() as u32,
        align,
    };
    // the package block of the runtime holds the body as stage 1 places it; stage 1 refuses a larger one at load
    if header.placed_bytes() > dkrt_sdk::PACKAGE_BYTES as u64 {
        return Err(err!(
            "the system package needs {} bytes in the package block (body header, image, the larger of bss and relocation table); the block holds {} (PACKAGE_BYTES in runtime/core/abi.toml)",
            header.placed_bytes(),
            dkrt_sdk::PACKAGE_BYTES
        ));
    }
    if let Some(&at) = linked.symbols.get(PLACED_SYMBOL) {
        let at = at as usize;
        linked
            .image
            .get(at..at + 4)
            .filter(|w| u32::from_be_bytes([w[0], w[1], w[2], w[3]]) == PLACED_UNSET)
            .ok_or_else(|| err!("{PLACED_SYMBOL} is not an unset word of the image"))?;
        // `placed_bytes` is at most PACKAGE_BYTES here, so it is never the unset value
        linked.image[at..at + 4].copy_from_slice(&(header.placed_bytes() as u32).to_be_bytes());
    }
    let mut out = header.encode().to_vec();
    debug_assert_eq!(out.len(), HEADER_BYTES);
    out.extend_from_slice(&linked.image);
    for r in &relocs {
        out.extend_from_slice(&r.to_be_bytes());
    }
    // the same checks the loader makes
    BodyHeader::decode(&out).map_err(|e| err!("the body does not decode: {e:?}"))?;
    Ok((out, (table >> 16) as u16, (table & 0xffff) as u16))
}

/// The package file built from the package object `obj`.
pub fn build(obj: &Dko) -> Result<Vec<u8>> {
    let (body, major, minor) = body(obj)?;
    pack(&body, major, minor)
}

/// The package object with the object `part` linked into it: its sections, symbols and relocations are added, and a symbol the part needs that the package defines is resolved.
/// A section or a defined symbol that both have is an error (two owners for one name).
pub fn with_part(obj: &Dko, part: &Dko) -> Result<Dko> {
    if obj.arch != part.arch {
        return Err(err!(
            "a package part is {} code, the package is {}",
            part.arch,
            obj.arch
        ));
    }
    let mut out = obj.clone();
    let have: std::collections::BTreeSet<&str> =
        obj.sections.iter().map(|s| s.name.as_str()).collect();
    if let Some(s) = part
        .sections
        .iter()
        .find(|s| have.contains(s.name.as_str()))
    {
        return Err(err!("section {} is in the package and in the part", s.name));
    }
    let defined: std::collections::BTreeSet<&str> = obj
        .symbols
        .iter()
        .filter(|s| s.section.is_some())
        .map(|s| s.name.as_str())
        .collect();
    if let Some(s) = part
        .symbols
        .iter()
        .find(|s| s.section.is_some() && defined.contains(s.name.as_str()))
    {
        return Err(err!(
            "symbol {} is defined by the package and by the part",
            s.name
        ));
    }
    out.sections.extend(part.sections.iter().cloned());
    out.symbols.extend(part.symbols.iter().cloned());
    out.relocs.extend(part.relocs.iter().cloned());
    // a name that is defined now is no longer an undefined entry; the other undefined names are listed once
    let now: std::collections::BTreeSet<String> = out
        .symbols
        .iter()
        .filter(|s| s.section.is_some())
        .map(|s| s.name.clone())
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    out.symbols
        .retain(|s| s.section.is_some() || (!now.contains(&s.name) && seen.insert(s.name.clone())));
    Ok(out)
}

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

const USAGE: &str = "usage: patchkit syspkg pack BODY --out FILE --table MAJOR.MINOR\n       patchkit syspkg build OBJECT [--part OBJECT]... --out FILE";

pub fn run(args: &[String]) -> Result<()> {
    let (mut body, mut out, mut tab) = (None, None, None);
    let mut parts: Vec<&String> = vec![];
    let mut it = args.iter();
    let build_cmd = match it.next().map(String::as_str) {
        Some("pack") => false,
        Some("build") => true,
        _ => return Err(err!("{USAGE}")),
    };
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" => out = it.next(),
            "--table" => tab = it.next(),
            "--part" => parts.push(it.next().ok_or_else(|| err!("{USAGE}"))?),
            _ if body.is_none() => body = Some(a),
            _ => return Err(err!("{USAGE}")),
        }
    }
    let (Some(body), Some(out)) = (body, out) else {
        return Err(err!("{USAGE}"));
    };
    let (file, major, minor) = if build_cmd {
        if tab.is_some() {
            return Err(err!(
                "syspkg build takes the table version from the package"
            ));
        }
        let text = std::fs::read_to_string(body).map_err(|e| err!("{body}: {e}"))?;
        let mut obj = Dko::parse(&text).map_err(|e| err!("{body}: {e}"))?;
        for p in &parts {
            let text = std::fs::read_to_string(p).map_err(|e| err!("{p}: {e}"))?;
            let part = Dko::parse(&text).map_err(|e| err!("{p}: {e}"))?;
            obj = with_part(&obj, &part).map_err(|e| err!("{p}: {e}"))?;
        }
        let (b, major, minor) = self::body(&obj).map_err(|e| err!("{body}: {e}"))?;
        (pack(&b, major, minor)?, major, minor)
    } else {
        if !parts.is_empty() {
            return Err(err!("--part belongs to syspkg build"));
        }
        let Some(tab) = tab else {
            return Err(err!("{USAGE}"));
        };
        let (major, minor) = table(tab)?;
        let bytes = std::fs::read(body).map_err(|e| err!("{body}: {e}"))?;
        (pack(&bytes, major, minor)?, major, minor)
    };
    if let Some(dir) = std::path::Path::new(out).parent() {
        std::fs::create_dir_all(dir).map_err(|e| err!("{}: {e}", dir.display()))?;
    }
    std::fs::write(out, &file).map_err(|e| err!("{out}: {e}"))?;
    println!("{out}: {} bytes, table {major}.{minor}", file.len());
    Ok(())
}
