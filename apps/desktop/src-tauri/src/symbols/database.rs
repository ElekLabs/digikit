use std::{collections::BTreeSet, path::Path};

use device_profile::Registry;
use dkaddrmap::{Cpu, Entry, Layer};
use rusqlite::{Connection, OpenFlags, Row};

use super::{SymbolMap, make_map, unique_port, verify_hash};

pub(super) fn read(path: &Path, cpu: Cpu) -> Result<Vec<SymbolMap>, String> {
    let mut db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(sql_error)?;
    db.execute_batch("PRAGMA trusted_schema = OFF; PRAGMA query_only = ON;")
        .map_err(sql_error)?;
    // Read the identity and rows from the same snapshot if a builder is updating the database.
    let tx = db.transaction().map_err(sql_error)?;
    let maps = read_maps(&tx, cpu)?;
    tx.commit().map_err(sql_error)?;
    Ok(maps)
}

fn read_maps(db: &Connection, cpu: Cpu) -> Result<Vec<SymbolMap>, String> {
    let tables: u32 = db
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_list \
             WHERE schema = 'main' AND type = 'table' AND name IN ('meta', 'functions')",
            [],
            |row| row.get(0),
        )
        .map_err(sql_error)?;
    if tables != 2 {
        return Err("expected a firmware database with meta and functions tables".into());
    }
    let dsp_registry = (cpu == Cpu::Sharc)
        .then(Registry::embedded)
        .transpose()
        .map_err(|e| e.to_string())?;
    let mut images = db
        .prepare("SELECT image FROM main.functions UNION SELECT image FROM main.meta ORDER BY 1")
        .map_err(sql_error)?;
    let mut rows = images.query([]).map_err(sql_error)?;
    let mut verified = BTreeSet::new();
    let mut maps = Vec::new();
    while let Some(row) = rows.next().map_err(sql_error)? {
        let image: String = row.get(0).map_err(sql_error)?;
        let Some(port) = unique_port(|p| match &dsp_registry {
            None => p.id == image.strip_suffix("-emac").unwrap_or(&image),
            Some(registry) => registry
                .dsp_for(p.device, p.firmware)
                .is_some_and(|dsp| dsp.image == image),
        })?
        else {
            continue;
        };
        // The source digest is a MAIN OS or DSP section hash, not the stock SysEx hash.
        // Different stock releases can share a section hash, so the internal ID must agree too.
        if meta(db, &image, "image")?.as_deref() != Some(&image) {
            return Err(format!(
                "{image}: missing or mismatched internal image identity"
            ));
        }
        let expected = match cpu {
            Cpu::Cf => port.main,
            Cpu::Sharc => {
                if meta(db, &image, "cpu")?.is_some_and(|declared| declared != "sharc") {
                    return Err(format!("{image}: expected a SHARC database"));
                }
                port.dsp
                    .ok_or_else(|| format!("{image}: port has no DSP fingerprint"))?
            }
        };
        verify_hash(
            meta(db, &image, "image_sha256")?.as_deref(),
            expected,
            &image,
        )?;
        if !verified.insert(port.id) {
            return Err(format!(
                "{image}: multiple internal image IDs resolve to {}",
                port.id
            ));
        }
        maps.push(make_map(port, entries(db, cpu, &image)?)?);
    }
    if maps.is_empty() {
        return Err("database has no firmware images with verified ports".into());
    }
    Ok(maps)
}

fn meta(db: &Connection, image: &str, key: &str) -> Result<Option<String>, String> {
    let mut stmt = db
        .prepare("SELECT value FROM main.meta WHERE image = ?1 AND key = ?2 LIMIT 2")
        .map_err(sql_error)?;
    let mut rows = stmt.query([image, key]).map_err(sql_error)?;
    let value = rows
        .next()
        .map_err(sql_error)?
        .map(|row| row.get(0))
        .transpose()
        .map_err(sql_error)?;
    if rows.next().map_err(sql_error)?.is_some() {
        return Err(format!("{image}: multiple database values for {key}"));
    }
    Ok(value)
}

fn entries(db: &Connection, cpu: Cpu, image: &str) -> Result<Vec<Entry>, String> {
    // These columns and the end-exclusive ranges are the ones used by addrmap_export.py.
    let sql = match cpu {
        Cpu::Cf => {
            "SELECT entry, end, name, NULL FROM main.functions WHERE image = ?1 AND end > entry"
        }
        Cpu::Sharc => {
            "SELECT entry_sw, end_sw, name, label FROM main.functions \
             WHERE image = ?1 AND end_sw > entry_sw"
        }
    };
    let mut stmt = db.prepare(sql).map_err(sql_error)?;
    let rows = stmt
        .query_map([image], |row| entry(row, cpu))
        .map_err(sql_error)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(sql_error)
}

fn entry(row: &Row<'_>, cpu: Cpu) -> rusqlite::Result<Entry> {
    let addr: u64 = row.get(0)?;
    let end: u64 = row.get(1)?;
    let name: Option<String> = row.get(2)?;
    let label: Option<String> = row.get(3)?;
    let name = name
        .filter(|s| !s.is_empty())
        .or_else(|| label.filter(|s| !s.is_empty()))
        .unwrap_or_else(|| match cpu {
            Cpu::Cf => format!("FUN_{addr:08x}"),
            Cpu::Sharc => format!("FUN_{addr:06x}"),
        });
    Ok(Entry {
        cpu,
        addr,
        size: end - addr,
        name,
        layer: Layer::Fw,
        module: None,
    })
}

fn sql_error(error: rusqlite::Error) -> String {
    format!("firmware database: {error}")
}
