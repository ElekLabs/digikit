//! `patchkit::syspkg::pack` writes what the runtime's header type reads back: the header fields, the body after it, and a digest that matches the body.

use dkrt_sdk::syspkg::{HEADER_BYTES, Header};
use patchkit::syspkg::{pack, validate};
use sha2::{Digest, Sha256};

#[test]
fn a_packed_file_reads_back_through_the_shared_header() {
    let body: Vec<u8> = (0..300).map(|i| i as u8).collect();
    let file = pack(&body, 2, 5).unwrap();
    let h = Header::decode(&file).unwrap();
    assert_eq!(
        (h.table_major(), h.table_minor(), h.size() as usize),
        (2, 5, body.len())
    );
    assert_eq!(&file[HEADER_BYTES..], &body[..]);
    let digest: [u8; 32] = Sha256::digest(&body).into();
    assert!(h.body_matches(body.len() as u32, &digest));
    // the table version it was built against decides who can load it
    assert!(h.compatible(2, 5) && h.compatible(2, 6) && !h.compatible(2, 4) && !h.compatible(3, 5));
}

#[test]
fn artifact_validation_refuses_truncation_and_corruption() {
    let mut file = pack(b"package body", 2, 5).unwrap();
    assert!(validate(&file, (2, 5)).is_ok());
    assert!(validate(&file[..file.len() - 1], (2, 5)).is_err());
    file[HEADER_BYTES] ^= 1;
    assert!(validate(&file, (2, 5)).is_err());
}

#[test]
fn artifact_validation_requires_a_compatible_resident_table() {
    let file = pack(b"package body", 2, 5).unwrap();
    assert!(validate(&file, (2, 6)).is_ok());
    for resident in [(3, 5), (2, 4)] {
        let error = validate(&file, resident).unwrap_err().to_string();
        assert!(error.contains("requires resident table 2.5"), "{error}");
    }
}

mod build {
    use dkrt_sdk::syspkg::body::{BodyHeader, HEADER_BYTES as BODY_HEADER, relocate};
    use dkrt_sdk::syspkg::{HEADER_BYTES, Header};
    use patchkit::dko::{Dko, DkoReloc, DkoSection, DkoSymbol, FORMAT};
    use patchkit::syspkg::{ENTRY, TABLE_SYMBOL, build};

    fn section(name: &str, kind: &str, bytes: &[u8]) -> DkoSection {
        DkoSection {
            name: name.into(),
            kind: kind.into(),
            align: 4,
            size: if kind == "bss" {
                20
            } else {
                bytes.len() as u32
            },
            hex: if kind == "bss" {
                String::new()
            } else {
                patchkit::error::hex(bytes)
            },
        }
    }

    fn symbol(name: &str, section: Option<&str>, value: u64) -> DkoSymbol {
        DkoSymbol {
            name: name.into(),
            section: section.map(String::from),
            value,
            global: true,
        }
    }

    fn reloc(section: &str, offset: u64, symbol: &str) -> DkoReloc {
        DkoReloc {
            section: section.into(),
            offset,
            rtype: "cf.32".into(),
            symbol: symbol.into(),
            addend: 0,
        }
    }

    /// An entry function (`rts`), the table word 1.2 and a pointer from data to the entry and one to the bss.
    fn package() -> Dko {
        let mut data = [0u8; 12];
        data[0..4].copy_from_slice(&0x0001_0002u32.to_be_bytes());
        Dko {
            format: FORMAT,
            arch: "cf".into(),
            source_sha256: "0".repeat(64),
            toolchain: Default::default(),
            sections: vec![
                section(".text", "text", &[0x4e, 0x75, 0x4e, 0x71]),
                section(".data", "data", &data),
                section(".bss", "bss", &[]),
            ],
            symbols: vec![
                symbol(ENTRY, Some(".text"), 0),
                symbol(TABLE_SYMBOL, Some(".data"), 0),
                symbol("counter", Some(".bss"), 4),
            ],
            relocs: vec![reloc(".data", 4, ENTRY), reloc(".data", 8, "counter")],
        }
    }

    #[test]
    fn the_built_file_carries_the_table_version_the_package_holds_and_a_body_that_relocates() {
        let file = build(&package()).unwrap();
        let h = Header::decode(&file).unwrap();
        assert_eq!((h.table_major(), h.table_minor()), (1, 2));
        let body = &file[HEADER_BYTES..];
        let b = BodyHeader::decode(body).unwrap();
        assert_eq!((b.entry, b.bss, b.relocs), (0, 20, 2));
        let mut image = body[BODY_HEADER..BODY_HEADER + b.image as usize].to_vec();
        relocate(&mut image, b.reloc_table(body), 0x1000).unwrap();
        let word = |at: usize| u32::from_be_bytes(image[at..at + 4].try_into().unwrap());
        // data starts after the 4-byte text: its pointer to the entry is the entry's address, its pointer to the bss lies behind the image
        let data = 4;
        assert_eq!(word(data + 4), 0x1000);
        assert_eq!(word(data + 8), 0x1000 + b.image + 4);
    }

    #[test]
    fn a_package_larger_than_the_package_block_is_refused_and_one_that_fills_it_is_built() {
        // placed bytes: 32 header + image + the larger of bss and relocation table; the fixture image is 16 bytes
        let with_bss = |bss: u32| {
            let mut o = package();
            o.sections
                .iter_mut()
                .find(|s| s.kind == "bss")
                .unwrap()
                .size = bss;
            o
        };
        let fits = dkrt_sdk::PACKAGE_BYTES as u32 - 32 - 16;
        assert!(
            build(&with_bss(fits)).is_ok(),
            "a package that fills the block exactly is built"
        );
        let e = build(&with_bss(fits + 1)).unwrap_err().to_string();
        assert!(
            e.contains("package block") && e.contains("PACKAGE_BYTES"),
            "{e}"
        );
    }

    #[test]
    fn a_symbol_the_package_does_not_define_is_refused() {
        let mut o = package();
        o.symbols.push(symbol("dk_ui_menu_new", None, 0));
        o.relocs.push(reloc(".data", 0, "dk_ui_menu_new"));
        let e = build(&o).unwrap_err().to_string();
        assert!(
            e.contains("links only through the resident table") && e.contains("dk_ui_menu_new"),
            "{e}"
        );
    }

    #[test]
    fn a_package_without_its_entry_or_table_word_is_refused() {
        let mut o = package();
        o.symbols.retain(|s| s.name != ENTRY);
        o.relocs.retain(|r| r.symbol != ENTRY);
        assert!(build(&o).unwrap_err().to_string().contains(ENTRY));
        let mut o = package();
        o.symbols.retain(|s| s.name != TABLE_SYMBOL);
        assert!(build(&o).unwrap_err().to_string().contains(TABLE_SYMBOL));
    }
}

/// The committed package object (`runtime/core/build/package.dko.json`) linked at two bases: the image at the second base is exactly the image at 0 with the base added at
/// each relocation entry of the body. A word that holds an address of the package and has no relocation entry (a relocation type the body skips, or a value the compiler
/// folded) would differ between the two links without an entry; an entry on a word that does not hold an address would be changed by the relocation and not by the link.
mod artifact {
    use dkrt_sdk::syspkg::body::{BodyHeader, HEADER_BYTES, relocate};
    use patchkit::dko::Dko;
    use patchkit::link::link;
    use patchkit::syspkg::body;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    const BASE: u64 = 0x0001_0000;

    /// The committed package object with the DIGIKIT menu linked into it, as `patchkit syspkg build --part` makes the artifact.
    fn package_object() -> Dko {
        super::menu_part::linked()
    }

    #[test]
    fn saving_firmware_pairs_it_with_the_cli_built_package() {
        use patchkit::syspkg::{build_from_repo, save_firmware};
        use std::fs;
        use std::process::Command;

        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dir = std::env::temp_dir().join(format!("pk-syspkg-test-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let cli_path = dir.join("system.dksp");
        let cli = Command::new(env!("CARGO_BIN_EXE_patchkit"))
            .current_dir(&root)
            .args([
                "syspkg",
                "build",
                "runtime/core/build/package.dko.json",
                "--part",
                "runtime/apps/digikit-menu/build/digikit-menu.dko.json",
                "--out",
            ])
            .arg(&cli_path)
            .output()
            .unwrap();
        assert!(
            cli.status.success(),
            "{}",
            String::from_utf8_lossy(&cli.stderr)
        );
        let expected = fs::read(cli_path).unwrap();
        let artifact = build_from_repo(&root).unwrap();
        assert_eq!(artifact.bytes(), expected);
        let metadata = artifact.metadata();
        let header = dkrt_sdk::syspkg::Header::decode(&expected).unwrap();
        assert_eq!(
            metadata.table(),
            (header.table_major(), header.table_minor())
        );
        assert_eq!(metadata.bytes(), expected.len());

        let image = dir.join("updated.os.syx");
        let companion = dir.join("updated.os.dksp");
        assert_eq!(
            save_firmware(&image, b"patched image", Some(&artifact)).unwrap(),
            Some(companion.clone())
        );
        assert_eq!(fs::read(&image).unwrap(), b"patched image");
        assert_eq!(fs::read(&companion).unwrap(), expected);
        assert!(save_firmware(&companion, b"wrong path", Some(&artifact)).is_err());
        assert_eq!(fs::read(&companion).unwrap(), expected);

        #[cfg(unix)]
        {
            let aliased = dir.join("aliased.syx");
            let aliased_companion = aliased.with_extension("dksp");
            fs::hard_link(&image, &aliased).unwrap();
            fs::hard_link(&image, &aliased_companion).unwrap();
            assert!(save_firmware(&aliased, b"wrong path", Some(&artifact)).is_err());
            assert_eq!(fs::read(&aliased).unwrap(), b"patched image");

            let missing_image = dir.join("missing.syx");
            std::os::unix::fs::symlink("missing.syx", missing_image.with_extension("dksp"))
                .unwrap();
            assert!(save_firmware(&missing_image, b"wrong path", Some(&artifact)).is_err());
            assert!(!missing_image.exists());
        }

        let blocked_image = dir.join("blocked-image.syx");
        let previous_package = blocked_image.with_extension("dksp");
        fs::create_dir(&blocked_image).unwrap();
        fs::write(&previous_package, b"previous system package").unwrap();
        assert!(save_firmware(&blocked_image, b"new image", Some(&artifact)).is_err());
        assert!(blocked_image.is_dir());
        assert_eq!(
            fs::read(previous_package).unwrap(),
            b"previous system package"
        );

        let previous_image = dir.join("blocked-package.syx");
        let blocked_package = previous_image.with_extension("dksp");
        fs::write(&previous_image, b"previous image").unwrap();
        fs::create_dir(&blocked_package).unwrap();
        assert!(save_firmware(&previous_image, b"new image", Some(&artifact)).is_err());
        assert_eq!(fs::read(previous_image).unwrap(), b"previous image");
        assert!(blocked_package.is_dir());

        let stock = dir.join("stock.syx");
        assert_eq!(save_firmware(&stock, b"stock image", None).unwrap(), None);
        assert_eq!(fs::read(&stock).unwrap(), b"stock image");
        assert!(!stock.with_extension("dksp").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reserves_case_variant_package_paths_before_they_exist() {
        use patchkit::syspkg::{companion_path, paths_alias};

        let root = tempfile::tempdir().unwrap();
        let companion = companion_path(&root.path().join("out.syx")).unwrap();
        let reserved = root.path().join("./OUT.DKSP");
        assert!(!companion.exists());
        assert!(!reserved.exists());
        assert!(paths_alias(&companion, &reserved));
    }

    #[test]
    fn native_validation_rejects_a_body_that_stage_one_cannot_place() {
        use dkrt_abi::resident::{RESIDENT_MAJOR, RESIDENT_MINOR};
        use patchkit::syspkg::{Artifact, pack};
        let file = pack(b"not a linked body", RESIDENT_MAJOR, RESIDENT_MINOR).unwrap();
        assert!(
            Artifact::from_bytes(file)
                .unwrap_err()
                .to_string()
                .contains("invalid system package body")
        );
    }

    #[test]
    fn every_absolute_address_of_the_package_image_has_a_relocation_entry() {
        let obj = package_object();
        let (bytes, _, _) = body(&obj).unwrap();
        let h = BodyHeader::decode(&bytes).unwrap();
        let mut image = bytes[HEADER_BYTES..HEADER_BYTES + h.image as usize].to_vec();
        let at_zero = link(&obj, 0, &BTreeMap::new()).unwrap();
        let at_base = link(&obj, BASE, &BTreeMap::new()).unwrap();
        assert_eq!(image, at_zero.image, "the body's image is the link at 0");
        relocate(&mut image, h.reloc_table(&bytes), BASE as u32).unwrap();
        let differing: Vec<usize> = (0..image.len())
            .filter(|&i| image[i] != at_base.image[i])
            .collect();
        assert!(
            differing.is_empty(),
            "{} byte(s) of the image differ between the relocated body and a link at {BASE:#x}, first at {:#x}: an address without a relocation entry, or an entry on a word that holds none",
            differing.len(),
            differing[0]
        );
        assert!(h.relocs > 1000, "the package has many addresses of its own");
    }
}

/// The DIGIKIT menu (C, `runtime/apps/digikit-menu`) as a part of the package: the committed objects of the package and of the menu, linked by `syspkg build --part`. One case that
/// fits (every symbol the menu needs is one the package defines, and its entry is inside the image) and the refusals of the part (a missing symbol, a name defined twice).
mod menu_part {
    use patchkit::dko::Dko;
    use patchkit::link::link;
    use patchkit::syspkg::{body, with_part};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn read(rel: &str) -> Dko {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(rel);
        Dko::parse(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())))
            .unwrap()
    }

    pub fn package() -> Dko {
        read("runtime/core/build/package.dko.json")
    }

    pub fn menu() -> Dko {
        read("runtime/apps/digikit-menu/build/digikit-menu.dko.json")
    }

    pub fn linked() -> Dko {
        with_part(&package(), &menu()).unwrap()
    }

    fn needs(o: &Dko) -> Vec<String> {
        let mut v: Vec<String> = o
            .symbols
            .iter()
            .filter(|s| s.section.is_none())
            .map(|s| s.name.clone())
            .collect();
        v.sort();
        v
    }

    /// The `dkrt_*` exports of abi.toml.
    fn exports() -> Vec<String> {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../runtime/core/abi.toml");
        let t: toml::Table = std::fs::read_to_string(p).unwrap().parse().unwrap();
        t["fn"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["name"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn the_menu_links_into_the_package_and_every_symbol_it_needs_is_one_the_package_defines() {
        let menu = menu();
        let needed = needs(&menu);
        assert!(
            needed.iter().any(|n| n.starts_with("dkrt_")),
            "the menu calls the runtime: {needed:?}"
        );
        // a runtime call of the menu is an export of abi.toml; the rest are core/ui calls
        let exported = exports();
        for n in &needed {
            assert!(
                n.starts_with("dk_ui_") || exported.contains(n),
                "{n} is neither a core/ui call nor an export of abi.toml"
            );
        }
        // the package alone does not define them (it would link without the menu, and have no menu); with the part every one is defined
        let package = package();
        assert!(
            body(&package).is_err(),
            "the package needs the menu's entry: its hook points at dk_app_settings_rows"
        );
        let both = with_part(&package, &menu).unwrap();
        let (bytes, _, _) =
            body(&both).expect("the package with the menu has no symbol left to resolve");
        let defined: Vec<&str> = both
            .symbols
            .iter()
            .filter(|s| s.section.is_some())
            .map(|s| s.name.as_str())
            .collect();
        for n in &needed {
            assert!(defined.contains(&n.as_str()), "{n}");
        }
        // the menu's hook function lies inside the image
        let at = link(&both, 0, &BTreeMap::new()).unwrap();
        let hook = at.symbols["dk_app_settings_rows"] as usize;
        assert!(hook < at.image.len() && bytes.len() > at.image.len());
    }

    #[test]
    fn a_symbol_of_the_menu_that_the_package_does_not_define_is_refused() {
        let mut package = package();
        package.symbols.retain(|s| s.name != "dkrt_boot_info");
        let e = body(&with_part(&package, &menu()).unwrap())
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("links only through the resident table") && e.contains("dkrt_boot_info"),
            "{e}"
        );
    }

    #[test]
    fn a_part_that_repeats_a_section_or_a_symbol_of_the_package_is_refused() {
        let package = package();
        let again = with_part(&package, &menu()).unwrap();
        let e = with_part(&again, &menu()).unwrap_err().to_string();
        assert!(e.contains("is in the package and in the part"), "{e}");
        // a symbol of the package defined again in a section of its own
        let mut clash = menu();
        clash.sections.truncate(1);
        clash.sections[0].name = ".text.clash".into();
        clash.relocs.clear();
        let mut sym = clash
            .symbols
            .iter()
            .find(|s| s.section.is_some())
            .unwrap()
            .clone();
        sym.name = patchkit::syspkg::ENTRY.into();
        sym.section = Some(".text.clash".into());
        clash.symbols = vec![sym];
        let e = with_part(&package, &clash).unwrap_err().to_string();
        assert!(e.contains("defined by the package and by the part"), "{e}");
    }
}
