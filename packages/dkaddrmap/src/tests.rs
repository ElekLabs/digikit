use super::*;

fn e(cpu: Cpu, addr: u64, size: u64, name: &str, layer: Layer, module: Option<&str>) -> Entry {
    Entry {
        cpu,
        addr,
        size,
        name: name.into(),
        layer,
        module: module.map(Into::into),
    }
}

fn key(image: &str) -> Key {
    Key {
        image_sha256: Some(image.into()),
        ..Key::default()
    }
}

fn fixture() -> (Vec<AddrMap>, LoadTable) {
    let ship = AddrMap::new(
        "ship",
        key("aa"),
        vec![
            e(Cpu::Cf, 0x2000_0000, 0x100, "dkrt_boot", Layer::Our, None),
            e(Cpu::Cf, 0x2000_0100, 0x40, "dkrt_tick", Layer::Our, None),
            e(
                Cpu::Cf,
                0x4000_1000,
                0x20,
                "cf.draw_text",
                Layer::Anchor,
                None,
            ),
        ],
    );
    let module = AddrMap::new(
        "module",
        Key {
            module: Some("test/demo".into()),
            ..Key::default()
        },
        vec![e(
            Cpu::Cf,
            0x30,
            0x10,
            "demo_tick",
            Layer::Our,
            Some("test/demo"),
        )],
    );
    let dev = AddrMap::new(
        "dev",
        key("aa"),
        vec![
            e(Cpu::Cf, 0x4000_0f00, 0x400, "FUN_40000f00", Layer::Fw, None),
            e(Cpu::Cf, 0x4000_2000, 0x10, "FUN_40002000", Layer::Fw, None),
            e(Cpu::Sharc, 0xb8_0000, 0x100, "FUN_b80000", Layer::Fw, None),
        ],
    );
    let loads = LoadTable {
        modules: vec![Load {
            id: "test/demo".into(),
            base: 0x2100_0000,
            size: 0x200,
        }],
    };
    (vec![ship, module, dev], loads)
}

fn name(r: &Resolver, cpu: Cpu, addr: u64) -> Option<String> {
    r.lookup(cpu, addr).map(|h| h.text())
}

#[test]
fn each_layer_names_its_addresses() {
    let (maps, loads) = fixture();
    let r = Resolver::new(&maps, &loads).unwrap();
    assert_eq!(name(&r, Cpu::Cf, 0x2000_0100), Some("dkrt_tick".into()));
    assert_eq!(name(&r, Cpu::Cf, 0x2000_0104), Some("dkrt_tick+0x4".into()));
    assert_eq!(
        name(&r, Cpu::Cf, 0x4000_1008),
        Some("cf.draw_text+0x8".into())
    );
    assert_eq!(
        name(&r, Cpu::Cf, 0x4000_2004),
        Some("FUN_40002000+0x4".into())
    );
    assert_eq!(
        name(&r, Cpu::Sharc, 0xb8_0010),
        Some("FUN_b80000+0x10".into())
    );
}

#[test]
fn most_specific_layer_wins_inside_a_larger_range() {
    let (maps, loads) = fixture();
    let r = Resolver::new(&maps, &loads).unwrap();
    // the anchor lies inside the firmware function FUN_40000f00
    let hit = r.lookup(Cpu::Cf, 0x4000_1000).unwrap();
    assert_eq!(
        (hit.layer, hit.name.as_str()),
        (Layer::Anchor, "cf.draw_text")
    );
    // next to the anchor the firmware name answers
    let hit = r.lookup(Cpu::Cf, 0x4000_1020).unwrap();
    assert_eq!((hit.layer, hit.name.as_str()), (Layer::Fw, "FUN_40000f00"));
}

#[test]
fn module_entries_follow_the_load_address() {
    let (maps, loads) = fixture();
    let r = Resolver::new(&maps, &loads).unwrap();
    let hit = r.lookup(Cpu::Cf, 0x2100_0034).unwrap();
    assert_eq!(
        (hit.text(), hit.module.as_deref(), hit.sym_addr),
        ("demo_tick+0x4".into(), Some("test/demo"), 0x2100_0030)
    );
    let moved = LoadTable {
        modules: vec![Load {
            id: "test/demo".into(),
            base: 0x2200_0000,
            size: 0x200,
        }],
    };
    let r = Resolver::new(&maps, &moved).unwrap();
    assert_eq!(name(&r, Cpu::Cf, 0x2100_0034), None);
    assert_eq!(name(&r, Cpu::Cf, 0x2200_0030), Some("demo_tick".into()));
}

#[test]
fn a_module_that_is_not_loaded_has_no_names() {
    let (maps, _) = fixture();
    let r = Resolver::new(&maps, &LoadTable::default()).unwrap();
    assert_eq!(r.unloaded, vec!["test/demo".to_string()]);
    assert_eq!(name(&r, Cpu::Cf, 0x30), None);
}

#[test]
fn unknown_addresses_have_no_name() {
    let (maps, loads) = fixture();
    let r = Resolver::new(&maps, &loads).unwrap();
    assert_eq!(name(&r, Cpu::Cf, 0x1234_5678), None);
    assert_eq!(
        name(&r, Cpu::Cf, 0x2000_0140),
        None,
        "the end of a range is outside it"
    );
    assert_eq!(
        name(&r, Cpu::Sharc, 0x2000_0000),
        None,
        "the CPUs have their own address spaces"
    );
}

#[test]
fn a_return_address_looks_two_bytes_back() {
    let (maps, loads) = fixture();
    let r = Resolver::new(&maps, &loads).unwrap();
    // the call was the last instruction of dkrt_boot: the return address is the start of dkrt_tick
    assert_eq!(name(&r, Cpu::Cf, 0x2000_0100), Some("dkrt_tick".into()));
    let hit = r.lookup_return(Cpu::Cf, 0x2000_0100).unwrap();
    assert_eq!(hit.name, "dkrt_boot");
}

#[test]
fn maps_of_different_images_are_refused() {
    let (mut maps, loads) = fixture();
    maps[2].key.image_sha256 = Some("bb".into());
    assert!(
        Resolver::new(&maps, &loads)
            .unwrap_err()
            .contains("different firmware")
    );
}

#[test]
fn the_file_round_trips_and_is_canonical() {
    let (maps, _) = fixture();
    let text = maps[0].to_json();
    assert_eq!(AddrMap::parse(&text).unwrap().to_json(), text);
    let mut shuffled = maps[0].clone();
    shuffled.entries.reverse();
    assert_eq!(shuffled.to_json(), text);
    assert!(AddrMap::parse(&text.replace("dk-addrmap", "other")).is_err());
    assert!(AddrMap::parse(&text.replace("\"version\":1", "\"version\":2")).is_err());
}
