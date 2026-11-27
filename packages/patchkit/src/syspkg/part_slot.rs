//! The directory slot of a package part. A part that defines `DKPART_TABLE` (`dkrt_abi::part`) is found by the package through `DKPKG_PARTS`, an array of table pointers in the package's
//! data: `claim` adds a relocation that writes the table's address into the first free slot and renames the table symbol per slot, so several parts do not collide.

use crate::dko::{Dko, DkoReloc};
use crate::error::*;
use dkrt_abi::part::{DIRECTORY_SYMBOL, MAX_PARTS, TABLE_SYMBOL};

const POINTER: u64 = 4;

/// The part with its table symbol renamed and `package` with the slot relocation added; `None` when the part has no table (it is only linked).
pub(super) fn claim(package: &mut Dko, part: &Dko) -> Result<Option<Dko>> {
    let Some(table) = part
        .symbols
        .iter()
        .find(|s| s.name == TABLE_SYMBOL && s.section.is_some())
    else {
        return Ok(None);
    };
    let dir = package
        .symbols
        .iter()
        .find(|s| s.name == DIRECTORY_SYMBOL && s.section.is_some())
        .ok_or_else(|| {
            err!("the part defines {TABLE_SYMBOL} but the package has no {DIRECTORY_SYMBOL}")
        })?;
    let section = dir.section.clone().expect("filtered on a section");
    let first = dir.value + POINTER; // the magic comes first
    let used = package
        .relocs
        .iter()
        .filter(|r| {
            r.section == section
                && r.offset >= first
                && r.offset < first + MAX_PARTS as u64 * POINTER
        })
        .count();
    if used >= MAX_PARTS {
        return Err(err!("the package takes {MAX_PARTS} parts"));
    }
    let name = format!("{TABLE_SYMBOL}.{used}");
    let mut renamed = part.clone();
    for s in renamed.symbols.iter_mut().filter(|s| s.name == table.name) {
        s.name = name.clone();
    }
    for r in renamed.relocs.iter_mut().filter(|r| r.symbol == table.name) {
        r.symbol = name.clone();
    }
    // the section of the table is the one that holds a symbol of this slot's name: it is named per slot too, so two parts' tables do not meet
    if let Some(sec) = table.section.clone() {
        super::part_private::rename_section(&mut renamed, &sec, &format!("{sec}.{used}"));
    }
    package.relocs.push(DkoReloc {
        section,
        offset: first + used as u64 * POINTER,
        rtype: "cf.32".into(),
        symbol: name,
        addend: 0,
    });
    Ok(Some(renamed))
}
