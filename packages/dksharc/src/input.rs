//! What the analysis reads: the text of a module's DSP image as 16-bit parcels, its relocations, imports, entry points and text symbols.
//!
//! [`Input::from_dkds`] reads the DKDS section of a `.dkm` (layout: `packages/patchkit/src/dkmdsp.rs`) and the `.dko.json` of the DSP
//! object (symbols and the size of the text section). Other callers fill the struct themselves.

use serde::{Deserialize, Serialize};

/// First byte of L2 as the linker places a module (`L2_BYTE_BASE` of patchkit): the image is linked here.
pub const L2_BYTE_BASE: u32 = 0x2000_0000;
/// Short-word pc of the first byte of L2 (`L2_SW_BASE` of patchkit): text at image byte `b` has pc `L2_SW_BASE + b / 2`.
pub const L2_SW_BASE: u32 = 0x00B8_0000;

const MAGIC: u32 = 0x444b_4453; // "DKDS"
const HEADER: usize = 0x40;
const IMPORT_LEN: usize = 32;
const RELOC_LEN: usize = 8;

/// A relocation inside the text: it patches the instruction that starts at `parcel`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextReloc {
    /// Parcel index of the instruction from the start of the text.
    pub parcel: u32,
    /// 0: the 24-bit address field, 1: the 32-bit immediate.
    pub kind: u8,
    /// 0: a pc of this object, 1: a byte address of this object, 2: the addend of an import.
    pub class: u8,
    /// Index into [`Input::imports`] when `class` is 2.
    pub import: u32,
}

/// A symbol of the text section.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextSymbol {
    pub name: String,
    /// Parcel index from the start of the text.
    pub parcel: u32,
    pub global: bool,
}

/// An entry point the packer recorded (a voice's render or note-on routine, an effect's process routine).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// `render`, `note_on` or `fx_process`.
    pub role: String,
    pub parcel: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Input {
    /// The text as 16-bit parcels in boot-stream order (instruction word 0 first).
    pub text: Vec<u16>,
    /// Short-word pc of parcel 0; `L2_SW_BASE` for a module linked at the nominal base.
    pub base_sw: u32,
    pub relocs: Vec<TextReloc>,
    pub imports: Vec<String>,
    pub symbols: Vec<TextSymbol>,
    pub entries: Vec<Entry>,
}

#[derive(Deserialize)]
struct Dko {
    sections: Vec<DkoSection>,
    symbols: Vec<DkoSymbol>,
}
#[derive(Deserialize)]
struct DkoSection {
    name: String,
    kind: String,
    size: u32,
}
#[derive(Deserialize)]
struct DkoSymbol {
    name: String,
    section: Option<String>,
    value: u64,
    #[serde(default)]
    global: bool,
}

fn be32(b: &[u8], off: usize) -> Result<u32, String> {
    b.get(off..off + 4)
        .map(|s| u32::from_be_bytes(s.try_into().unwrap()))
        .ok_or_else(|| format!("DKDS: read at {off:#x} is past the end"))
}

fn span<'a>(
    b: &'a [u8],
    off: u32,
    count: u32,
    each: usize,
    what: &str,
) -> Result<&'a [u8], String> {
    let len = (count as usize)
        .checked_mul(each)
        .ok_or_else(|| format!("DKDS: {what} count"))?;
    b.get(off as usize..(off as usize).saturating_add(len))
        .ok_or_else(|| format!("DKDS: {what} table is past the end"))
}

impl Input {
    /// Read the DKDS section `dkds` (header at byte 0) and the DSP object `dko_json`. The text is the first `text` section of the object:
    /// the image starts with it, so its size says where the data begins.
    pub fn from_dkds(dkds: &[u8], dko_json: &[u8]) -> Result<Input, String> {
        if be32(dkds, 0)? != MAGIC {
            return Err("DKDS: bad magic".into());
        }
        let version = be32(dkds, 4)?;
        if !(1..=5).contains(&version) {
            return Err(format!("DKDS: version {version}"));
        }
        if dkds.len() < HEADER {
            return Err("DKDS: short header".into());
        }
        let image_size = be32(dkds, 0x0c)?;
        let image_off = be32(dkds, 0x18)?;
        let image = span(dkds, image_off, image_size, 1, "image")?;
        let dko: Dko = serde_json::from_slice(dko_json).map_err(|e| format!("dko: {e}"))?;
        let mut texts = dko.sections.iter().filter(|s| s.kind == "text");
        let text_sec = texts.next().ok_or("dko: no text section")?;
        if texts.next().is_some() {
            return Err("dko: more than one text section".into());
        }
        if text_sec.size % 2 != 0 || text_sec.size as usize > image.len() {
            return Err(format!(
                "dko: text size {} against image {}",
                text_sec.size,
                image.len()
            ));
        }
        let text: Vec<u16> = image[..text_sec.size as usize]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        let n_text = text.len() as u32;

        let imports = span(
            dkds,
            be32(dkds, 0x24)?,
            be32(dkds, 0x28)?,
            IMPORT_LEN,
            "import",
        )?
        .as_chunks::<IMPORT_LEN>()
        .0
        .iter()
        .map(|c| {
            let end = c.iter().position(|b| *b == 0).unwrap_or(IMPORT_LEN);
            String::from_utf8_lossy(&c[..end]).into_owned()
        })
        .collect::<Vec<_>>();

        let mut relocs = vec![];
        for r in span(
            dkds,
            be32(dkds, 0x1c)?,
            be32(dkds, 0x20)?,
            RELOC_LEN,
            "reloc",
        )?
        .as_chunks::<RELOC_LEN>()
        .0
        {
            let off = u32::from_be_bytes(r[..4].try_into().unwrap());
            let w = u32::from_be_bytes(r[4..].try_into().unwrap());
            if off % 2 != 0 {
                return Err(format!("DKDS: reloc at odd byte {off:#x}"));
            }
            if off / 2 >= n_text {
                continue; // a field of the data section
            }
            let import = w & 0x00ff_ffff;
            let class = ((w >> 24) & 0xf) as u8;
            if class == 2 && import as usize >= imports.len() {
                return Err(format!(
                    "DKDS: reloc at {off:#x}: import {import} of {}",
                    imports.len()
                ));
            }
            relocs.push(TextReloc {
                parcel: off / 2,
                kind: (w >> 28) as u8,
                class,
                import,
            });
        }

        let mut entries = vec![];
        if version >= 2 || be32(dkds, 0x30)? > 0 {
            let each = match version {
                1 => 16,
                2 => 48,
                _ => 64,
            };
            let voices = span(dkds, be32(dkds, 0x2c)?, be32(dkds, 0x30)?, each, "voice")?;
            for v in voices.chunks_exact(each) {
                let render = u32::from_be_bytes(v[4..8].try_into().unwrap());
                entries.push(("render", render));
                if version >= 3 {
                    let note_on = u32::from_be_bytes(v[8..12].try_into().unwrap());
                    if note_on != 0 {
                        entries.push(("note_on", note_on));
                    }
                }
            }
        }
        if version >= 4 {
            for f in span(dkds, be32(dkds, 0x34)?, be32(dkds, 0x38)?, 64, "fx")?
                .as_chunks::<64>()
                .0
            {
                entries.push(("fx_process", u32::from_be_bytes(f[..4].try_into().unwrap())));
            }
        }
        let entries = entries
            .into_iter()
            .map(|(role, off)| {
                if off % 2 != 0 || off / 2 >= n_text {
                    Err(format!("DKDS: {role} entry {off:#x} is outside the text"))
                } else {
                    Ok(Entry {
                        role: role.into(),
                        parcel: off / 2,
                    })
                }
            })
            .collect::<Result<Vec<_>, _>>()?;

        let symbols = dko
            .symbols
            .iter()
            .filter(|s| s.section.as_deref() == Some(text_sec.name.as_str()))
            .map(|s| {
                if s.value >= n_text as u64 {
                    Err(format!(
                        "dko: symbol {} at parcel {} is outside the text",
                        s.name, s.value
                    ))
                } else {
                    Ok(TextSymbol {
                        name: s.name.clone(),
                        parcel: s.value as u32,
                        global: s.global,
                    })
                }
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Input {
            text,
            base_sw: L2_SW_BASE,
            relocs,
            imports,
            symbols,
            entries,
        })
    }
}
