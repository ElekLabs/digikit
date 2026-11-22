//! `patchkit::syspkg::pack` writes what the runtime's header type reads back: the header fields, the body after it, and a digest that matches the body.

use dkrt_sdk::syspkg::{HEADER_BYTES, Header};
use patchkit::syspkg::pack;
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

    fn package_object() -> Dko {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../runtime/core/build/package.dko.json");
        Dko::parse(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    #[test]
    fn every_absolute_address_of_the_package_image_has_a_relocation_entry() {
        let obj = package_object();
        let (bytes, _, _) = body(&obj).unwrap();
        let h = BodyHeader::decode(&bytes).unwrap();
        let mut image = bytes[HEADER_BYTES..HEADER_BYTES + h.image as usize].to_vec();
        let at_zero = link(&obj, 0, &BTreeMap::new()).unwrap();
        assert_eq!(image, at_zero.image, "the body's image is the link at 0");
        relocate(&mut image, h.reloc_table(&bytes), BASE as u32).unwrap();
        let at_base = link(&obj, BASE, &BTreeMap::new()).unwrap();
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
