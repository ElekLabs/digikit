//! `patchkit store dump --json` end to end: a card image file, an overlay of the guest's writes, the device layout, the store header pair, the selection record and
//! the settings copies. The decoders have their own unit tests (src/storedump.rs); this checks the command puts the card's sectors together.
#![cfg(not(target_arch = "wasm32"))]

use dkrt_sdk::settings as set;
use emmc_card::CardOverlaySector;
use plusdrive_format::{
    StoreFile, Tree, TreeOptions, build_tree_image, card_layout, encode_overlay,
};
use serde_json::Value;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use std::process::Command;

const SECTOR: u64 = 512;

/// A runtime device with a card store, from the device registry.
fn dev() -> &'static str {
    patchkit::devdata::all()
        .iter()
        .filter(|d| d.digikit_id.is_some() && card_layout(&d.short).is_ok_and(|l| l.store.is_some()))
        .map(|d| d.short.as_str())
        .next()
        .expect("a runtime device with a card store")
}

fn dump(dir: &Path, image: &Path, overlay: Option<&Path>) -> Value {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_patchkit"));
    cmd.args(["store", "dump", "--json", "--device", dev()])
        .arg(image);
    if let Some(o) = overlay {
        cmd.arg("--overlay").arg(o);
    }
    let out = cmd.current_dir(dir).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

/// A card file of the device layout (sparse) with nothing in it, or with the store (two files) and a selection built by the image builder: only the sectors
/// of the store area are copied.
fn card(dir: &Path, with_store: bool) -> std::path::PathBuf {
    let layout = card_layout(dev()).unwrap();
    let range = layout.store.clone().unwrap();
    let path = dir.join("card.img");
    let mut file = std::fs::File::create(&path).unwrap();
    file.set_len(range.end_sector * SECTOR).unwrap();
    if with_store {
        let tree = Tree {
            store: vec![
                StoreFile::file("a/one.dkm", vec![1; 700]),
                StoreFile::file("two.dkm", vec![2; 100]),
            ],
            selection: Some(vec!["two.dkm".to_string()]),
            ..Tree::default()
        };
        let built = build_tree_image(
            tree,
            TreeOptions {
                layout,
                ..TreeOptions::default()
            },
            None,
        )
        .unwrap();
        for (off, bytes) in built.image.ranges() {
            if *off >= range.first_sector * SECTOR {
                file.seek(SeekFrom::Start(*off)).unwrap();
                file.write_all(bytes).unwrap();
            }
        }
    }
    path
}

/// The sectors of a settings record in copy `copy` (0 = A, 1 = B), as an overlay: the record holds `key` = `value`, sequence number `seq`.
fn record(copy: u64, seq: u32, value: u8) -> Vec<CardOverlaySector> {
    let range = card_layout(dev()).unwrap().store.unwrap();
    let first = range.settings_a() + copy * u64::from(set::SECTORS);
    let mut b = vec![0u8; set::AREA];
    set::init(&mut b, seq);
    set::put(&mut b, b"fx/route/test/dyn-fx-gain", &[value]).unwrap();
    set::seal(&mut b, seq);
    b[..set::used(&b).div_ceil(512) * 512]
        .chunks(512)
        .enumerate()
        .map(|(i, c)| CardOverlaySector {
            sector: first + i as u64,
            written: [0xff; 64],
            data: c.try_into().unwrap(),
        })
        .collect()
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_card_without_a_header_pair_has_no_store() {
    let dir = scratch("store-dump-blank");
    let v = dump(&dir, &card(&dir, false), None);
    assert_eq!(v["device"], dev());
    assert!(v["store"].is_null(), "{}", v["store"]);
    assert!(v["selection"]["active"].is_null());
    assert!(v["settings"]["active"].is_null());
}

#[test]
fn the_store_the_selection_and_the_settings_copy_in_use_are_read_through_the_overlay() {
    let dir = scratch("store-dump-card");
    let image = card(&dir, true);
    // a write sequence of the runtime: A (1), B (2), A (3); the guest's writes are an overlay over the card
    let mut sectors = record(0, 3, 30);
    sectors.extend(record(1, 2, 20));
    sectors.sort_by_key(|s| s.sector);
    let overlay = dir.join("card.overlay");
    std::fs::write(&overlay, encode_overlay(&sectors)).unwrap();
    let v = dump(&dir, &image, Some(&overlay));

    let files: Vec<&str> = v["store"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(files.len(), 2);
    assert!(files.contains(&"a/one.dkm") && files.contains(&"two.dkm"));
    assert_eq!(v["selection"]["active"], "A");
    assert_eq!(v["selection"]["a"]["paths"][0], "two.dkm");
    assert_eq!(v["settings"]["active"], "A");
    let copies = v["settings"]["copies"].as_array().unwrap();
    assert_eq!(
        (copies[0]["seq"].as_u64(), copies[1]["seq"].as_u64()),
        (Some(3), Some(2))
    );
    assert_eq!(copies[0]["entries"][0]["key"], "fx/route/test/dyn-fx-gain");
    assert_eq!(copies[0]["entries"][0]["value_hex"], "1e");

    // without the overlay the card holds no settings record
    let v = dump(&dir, &image, None);
    assert!(v["settings"]["active"].is_null());
}
