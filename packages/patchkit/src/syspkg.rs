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
use std::path::Path;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

mod install_plan;
mod part_private;
mod part_slot;
pub use install_plan::{InstallAction, InstallRefusal, install_plan};

/// The entry function the package exports.
pub const ENTRY: &str = "dkpkg_entry";
/// The word the package holds: the resident table version it was built against, `major << 16 | minor`.
pub const TABLE_SYMBOL: &str = "DKPKG_TABLE";

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
    let linked = link::link(obj, 0, &BTreeMap::new())?;
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
    // a part that has a table (`dkrt_abi::part`) takes the next slot of the package's directory; its table symbol gets a name of its own
    let renamed;
    let part = match part_slot::claim(&mut out, part)? {
        Some(p) => {
            renamed = p;
            &renamed
        }
        None => part,
    };
    // a section of the part that only the part uses (the compiler's unnamed constants, `.rodata.global.N`) may share its name with one of the package: it gets a name of its own
    let renamed_private = part_private::rename(obj, part);
    let part = &renamed_private;
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

/// Check the built system package's resident-table requirement and body integrity.
pub fn validate(file: &[u8], resident: (u16, u16)) -> Result<()> {
    checked_header(file, resident).map(|_| ())
}

fn checked_header(file: &[u8], (resident_major, resident_minor): (u16, u16)) -> Result<Header> {
    let header = Header::decode(file).map_err(|e| err!("invalid system package header: {e:?}"))?;
    if !header.compatible(resident_major, resident_minor) {
        return Err(err!(
            "system package requires resident table {}.{}, this build provides {}.{}",
            header.table_major(),
            header.table_minor(),
            resident_major,
            resident_minor,
        ));
    }
    let body = &file[dkrt_sdk::syspkg::HEADER_BYTES..];
    if body.len() != header.size() as usize {
        return Err(err!("system package body size differs from its header"));
    }
    let digest: [u8; 32] = Sha256::digest(body).into();
    if !header.body_matches(header.size(), &digest) {
        return Err(err!("system package body checksum differs from its header"));
    }
    Ok(header)
}

/// A system package checked against this build's resident ABI.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
pub struct Artifact {
    bytes: Vec<u8>,
    header: Header,
}

#[cfg(not(target_arch = "wasm32"))]
pub struct Metadata {
    table: (u16, u16),
    bytes: usize,
}

#[cfg(not(target_arch = "wasm32"))]
impl Metadata {
    /// The packed runtime version the package is prepared for.
    pub fn runtime_version(&self) -> u32 {
        crate::dkm::runtime_version()
    }

    /// The package's required resident table major and minor.
    pub fn table(&self) -> (u16, u16) {
        self.table
    }

    /// The complete file size.
    pub fn bytes(&self) -> usize {
        self.bytes
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Artifact {
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        use dkrt_abi::resident::{RESIDENT_MAJOR, RESIDENT_MINOR};
        let header = checked_header(&bytes, (RESIDENT_MAJOR, RESIDENT_MINOR))?;
        let body = &bytes[dkrt_sdk::syspkg::HEADER_BYTES..];
        let placed =
            BodyHeader::decode(body).map_err(|e| err!("invalid system package body: {e:?}"))?;
        if placed.placed_bytes() > dkrt_sdk::PACKAGE_BYTES as u64 {
            return Err(err!(
                "the system package body does not fit in the package block"
            ));
        }
        let mut image = body[HEADER_BYTES..HEADER_BYTES + placed.image as usize].to_vec();
        dkrt_sdk::syspkg::body::relocate(&mut image, placed.reloc_table(body), 0)
            .map_err(|e| err!("invalid system package relocations: {e:?}"))?;
        Ok(Self { bytes, header })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn metadata(&self) -> Metadata {
        Metadata {
            table: (self.header.table_major(), self.header.table_minor()),
            bytes: self.bytes.len(),
        }
    }
}

/// Link the same committed system package and menu objects used by `core/runtime:package`.
#[cfg(not(target_arch = "wasm32"))]
pub fn build_from_repo(root: &Path) -> Result<Artifact> {
    build_from_runtime_object(root, &root.join("runtime/core/build/package.dko.json"))
}

/// Build the companion of the selected runtime object with the repository's menu object.
#[cfg(not(target_arch = "wasm32"))]
pub fn build_from_runtime_object(root: &Path, object: &Path) -> Result<Artifact> {
    let menu = root.join("runtime/apps/digikit-menu/build/digikit-menu.dko.json");
    let (bytes, _, _) = build_files(object, [menu.as_path()])?;
    Artifact::from_bytes(bytes)
}

/// Save an image and, when it contains the resident runtime, its sibling system package.
#[cfg(not(target_arch = "wasm32"))]
pub fn save_firmware(
    path: &Path,
    image: &[u8],
    package: Option<&Artifact>,
) -> Result<Option<PathBuf>> {
    use std::fs;

    let Some(package) = package else {
        fs::write(path, image).map_err(|e| err!("{}: {e}", path.display()))?;
        return Ok(None);
    };
    let companion = companion_path(path)?;
    let image_path = resolve_output(path).unwrap_or_else(|| path.to_path_buf());
    let companion_path = resolve_output(&companion).unwrap_or_else(|| companion.clone());
    let (staged_image, replaces_image) =
        stage_output(&image_path, image).map_err(|e| err!("{}: {e}", path.display()))?;
    let (staged_package, _) = stage_output(&companion_path, package.bytes())
        .map_err(|e| err!("{}: {e}", companion.display()))?;

    // Keep the old image available until the companion has been published.
    let backup = if replaces_image {
        let dir = tempfile::tempdir_in(output_parent(&image_path))
            .map_err(|e| err!("backing up {}: {e}", path.display()))?;
        let old = dir.path().join("image");
        fs::hard_link(&image_path, &old)
            .or_else(|_| fs::copy(&image_path, &old).map(|_| ()))
            .map_err(|e| err!("backing up {}: {e}", path.display()))?;
        Some(dir)
    } else {
        None
    };
    staged_image
        .persist(&image_path)
        .map_err(|e| err!("{}: {}", path.display(), e.error))?;
    if let Err(e) = staged_package.persist(&companion_path) {
        let restore = match &backup {
            Some(dir) => fs::rename(dir.path().join("image"), &image_path),
            None => fs::remove_file(&image_path),
        };
        if let Err(restore) = restore {
            let saved = backup.map(|dir| dir.keep().join("image"));
            let location = saved.map_or_else(String::new, |p| {
                format!("; previous image saved at {}", p.display())
            });
            return Err(err!(
                "{}: {}; restoring {} failed: {restore}{location}",
                companion.display(),
                e.error,
                path.display(),
            ));
        }
        return Err(err!("{}: {}", companion.display(), e.error));
    }
    Ok(Some(companion))
}

#[cfg(not(target_arch = "wasm32"))]
fn stage_output(path: &Path, bytes: &[u8]) -> std::io::Result<(tempfile::NamedTempFile, bool)> {
    use std::fs::{self, OpenOptions};
    use std::io::{Error, ErrorKind, Write};

    let previous = match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => {
            OpenOptions::new().write(true).open(path)?;
            Some(metadata.permissions())
        }
        Ok(_) => {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "output is not a regular file",
            ));
        }
        Err(e) if e.kind() == ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    let mut builder = tempfile::Builder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o666));
    }
    let mut staged = builder.tempfile_in(output_parent(path))?;
    staged.write_all(bytes)?;
    let replacing = previous.is_some();
    if let Some(permissions) = previous {
        staged.as_file().set_permissions(permissions)?;
    }
    staged.as_file().sync_all()?;
    Ok((staged, replacing))
}

/// Find the sibling system package output, rejecting paths that alias the image.
#[cfg(not(target_arch = "wasm32"))]
pub fn companion_path(image: &Path) -> Result<PathBuf> {
    let package = image.with_extension("dksp");
    if image
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("dksp"))
        || paths_alias(image, &package)
    {
        return Err(err!(
            "firmware and system package paths coincide at {}; choose a firmware output with a different extension",
            package.display()
        ));
    }
    Ok(package)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn paths_alias(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(a), Ok(b)) = (std::fs::metadata(a), std::fs::metadata(b)) {
            return a.dev() == b.dev() && a.ino() == b.ino();
        }
    }
    matches!((resolve_output(a), resolve_output(b)), (Some(a), Some(b))
        if a == b || (a.parent() == b.parent()
            && a.file_name().zip(b.file_name()).is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))))
}

#[cfg(not(target_arch = "wasm32"))]
fn output_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

#[cfg(not(target_arch = "wasm32"))]
fn resolve_output(path: &Path) -> Option<PathBuf> {
    let mut path = path.to_path_buf();
    for _ in 0..40 {
        if let Ok(existing) = path.canonicalize() {
            return Some(existing);
        }
        let parent = output_parent(&path);
        match std::fs::read_link(&path) {
            Ok(target) => path = parent.join(target),
            Err(_) => return Some(parent.canonicalize().ok()?.join(path.file_name()?)),
        }
    }
    None
}

fn build_files<'a>(
    object: &Path,
    parts: impl IntoIterator<Item = &'a Path>,
) -> Result<(Vec<u8>, u16, u16)> {
    let read_object = |path: &Path| {
        let text = std::fs::read_to_string(path).map_err(|e| err!("{}: {e}", path.display()))?;
        Dko::parse(&text).map_err(|e| err!("{}: {e}", path.display()))
    };
    let mut obj = read_object(object)?;
    for p in parts {
        let part = read_object(p)?;
        obj = with_part(&obj, &part).map_err(|e| err!("{}: {e}", p.display()))?;
    }
    let (body, major, minor) = body(&obj).map_err(|e| err!("{}: {e}", object.display()))?;
    Ok((pack(&body, major, minor)?, major, minor))
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
        build_files(Path::new(body), parts.iter().map(Path::new))?
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
