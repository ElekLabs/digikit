// device-literals: allow

use std::path::PathBuf;

use dkaddrmap::Cpu;
use rusqlite::Connection;
use serde_json::json;
use tempfile::TempDir;

use super::*;

fn port(id: &str) -> &'static PortFingerprint {
    unique_port(|p| p.id == id).unwrap().unwrap()
}

fn fixture(cpu: Cpu) -> (TempDir, PathBuf, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("renamed source.sqlite");
    let db = Connection::open(&path).unwrap();
    let functions = match cpu {
        Cpu::Cf => "image TEXT, entry INTEGER, end INTEGER, name TEXT",
        Cpu::Sharc => "image TEXT, entry_sw INTEGER, end_sw INTEGER, name TEXT, label TEXT",
    };
    db.execute_batch(&format!(
        "CREATE TABLE meta (image TEXT, key TEXT, value TEXT); CREATE TABLE functions ({functions});"
    ))
    .unwrap();
    (dir, path, db)
}

fn add_identity(db: &Connection, image: &str, hash: &str) {
    db.execute(
        "INSERT INTO meta VALUES (?1, 'image', ?1), (?1, 'image_sha256', ?2)",
        [image, hash],
    )
    .unwrap();
}

fn add_cf(db: &Connection, image: &str, hash: &str) {
    add_identity(db, image, hash);
    db.execute(
        "INSERT INTO functions VALUES (?1, 1073741824, 1073741830, 'cf_start')",
        [image],
    )
    .unwrap();
}

fn read_error(kind: SourceKind, path: &Path) -> String {
    match read_source(kind, path) {
        Ok(_) => panic!("invalid source was accepted"),
        Err(error) => error,
    }
}

#[test]
fn imports_each_verified_image_from_a_renamed_database() {
    let (_dir, path, db) = fixture(Cpu::Cf);
    let dn = port("dn2-1.12");
    let dt = port("dt2-1.17");
    add_cf(&db, "dn2-1.12-emac", dn.main);
    add_cf(
        &db,
        "dt2-1.17-emac",
        &format!(" {} ", dt.main.to_uppercase()),
    );
    add_cf(&db, "unknown-1.0-emac", dt.main);
    drop(db);
    let original = std::fs::read(&path).unwrap();

    let maps = read_source(SourceKind::Cf, &path).unwrap();
    assert_eq!(maps.len(), 2);
    for (map, port) in maps.iter().zip([dn, dt]) {
        assert_eq!(
            (map.device, map.firmware, map.sha256),
            (port.device, port.firmware, port.stock)
        );
        let parsed = AddrMap::parse(&map.json).unwrap();
        assert_eq!(parsed.key.image_sha256.as_deref(), Some(port.stock));
        assert_eq!(parsed.entries.len(), 1);
        let entry = &parsed.entries[0];
        assert_eq!(
            (entry.cpu, entry.addr, entry.size),
            (Cpu::Cf, 0x40000000, 6)
        );
    }
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn rejects_a_supported_image_with_the_wrong_or_missing_fingerprint() {
    let (_dir, path, db) = fixture(Cpu::Cf);
    let dt = port("dt2-1.17");
    add_cf(&db, "dt2-1.17-emac", dt.main);
    add_cf(&db, "dn2-1.12-emac", dt.main);

    assert!(read_error(SourceKind::Cf, &path).contains("SHA-256 does not match"));
    db.execute(
        "DELETE FROM meta WHERE image = 'dn2-1.12-emac' AND key = 'image_sha256'",
        [],
    )
    .unwrap();
    assert!(read_error(SourceKind::Cf, &path).contains("missing image SHA-256"));
}

#[test]
fn rejects_ambiguous_internal_image_ids() {
    let (_dir, path, db) = fixture(Cpu::Cf);
    let dt = port("dt2-1.17");
    add_cf(&db, "dt2-1.17-emac", dt.main);
    add_cf(&db, dt.id, dt.main);
    assert!(read_error(SourceKind::Cf, &path).contains("multiple internal image IDs"));
    db.execute(
        "DELETE FROM meta WHERE image = ?1 AND key = 'image'",
        [dt.id],
    )
    .unwrap();
    assert!(read_error(SourceKind::Cf, &path).contains("internal image identity"));
}

#[test]
fn keeps_sharc_word_ranges_and_uses_the_internal_release_for_a_shared_blob() {
    let (_dir, path, db) = fixture(Cpu::Sharc);
    let dn = port("dn2-1.12");
    let previous = port("dn2-1.11");
    assert_eq!(dn.dsp, previous.dsp);
    add_identity(&db, dn.id, dn.dsp.unwrap());
    db.execute(
        "INSERT INTO functions VALUES (?1, 12062720, 12062724, NULL, 'start'), \
            (?1, 12062724, 12062725, '', '')",
        [dn.id],
    )
    .unwrap();

    let maps = read_source(SourceKind::Sharc, &path).unwrap();
    assert_eq!(maps.len(), 1);
    assert_eq!(maps[0].sha256, dn.stock);
    let parsed = AddrMap::parse(&maps[0].json).unwrap();
    let first = &parsed.entries[0];
    assert_eq!(
        (first.cpu, first.addr, first.size, first.name.as_str()),
        (Cpu::Sharc, 0xb81000, 4, "start")
    );
    assert_eq!(parsed.entries[1].name, "FUN_b81004");
    assert!(read_error(SourceKind::Cf, &path).contains("SHA-256 does not match"));
}

#[test]
fn validates_direct_developer_maps_and_canonicalizes_the_stock_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("symbols.json");
    let port = port("dt2-1.17");
    let mut source = json!({
        "format": dkaddrmap::FORMAT,
        "version": dkaddrmap::VERSION,
        "kind": "dev",
        "key": {"device": port.device, "firmware": port.firmware, "image_sha256": port.stock.to_uppercase()},
        "entries": [{"cpu": "cf", "addr": 0x40000000, "size": 0, "name": "point", "layer": "fw"}],
    });
    let write = |value: &serde_json::Value| std::fs::write(&path, value.to_string()).unwrap();
    write(&source);
    let out = read_source(SourceKind::Map, &path).unwrap();
    assert_eq!(out[0].sha256, port.stock);
    assert_eq!(
        AddrMap::parse(&out[0].json)
            .unwrap()
            .key
            .image_sha256
            .as_deref(),
        Some(port.stock)
    );
    source["entries"][0]["layer"] = json!("anchor");
    write(&source);
    assert!(read_error(SourceKind::Map, &path).contains("fw layer"));
    source["entries"][0]["layer"] = json!("fw");
    source["kind"] = json!("ship");
    write(&source);
    assert!(read_error(SourceKind::Map, &path).contains("kind dev"));
    source["kind"] = json!("dev");
    source["key"]["image_sha256"] = json!(port.main);
    write(&source);
    assert!(read_error(SourceKind::Map, &path).contains("SHA-256 does not match"));
}

#[test]
#[ignore = "requires generated databases under DIGI_SYMBOL_DB_OUT"]
fn imports_the_current_repository_databases() {
    let out = PathBuf::from(std::env::var_os("DIGI_SYMBOL_DB_OUT").unwrap());
    for (kind, cpu, name, image) in [
        (
            SourceKind::Cf,
            Cpu::Cf,
            "cfdb/dn2-1.12-emac.sqlite",
            "dn2-1.12",
        ),
        (
            SourceKind::Cf,
            Cpu::Cf,
            "cfdb/dt2-1.17-emac.sqlite",
            "dt2-1.17",
        ),
        (
            SourceKind::Sharc,
            Cpu::Sharc,
            "sharcdb/dn2-1.12.sqlite",
            "dn2-1.12",
        ),
        (
            SourceKind::Sharc,
            Cpu::Sharc,
            "sharcdb/dt2-1.17.sqlite",
            "dt2-1.17",
        ),
    ] {
        let maps = read_source(kind, &out.join(name)).unwrap();
        assert_eq!(maps.len(), 1);
        let parsed = AddrMap::parse(&maps[0].json).unwrap();
        assert_eq!(maps[0].sha256, port(image).stock);
        assert!(parsed.entries.len() > 1000);
        assert!(parsed.entries.iter().all(|e| e.cpu == cpu && e.size > 0));
    }
}
