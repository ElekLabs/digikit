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
            dsp_base: None,
            fini: None,
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
            dsp_base: None,
            fini: None,
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

#[test]
fn module_dsp_entries_are_rebased_by_the_dsp_base() {
    let map = AddrMap::new(
        "module",
        Key::default(),
        vec![
            e(
                Cpu::Cf,
                0x30,
                0x10,
                "demo_tick",
                Layer::Our,
                Some("test/demo"),
            ),
            e(
                Cpu::Sharc,
                0x10,
                0x20,
                "demo_render",
                Layer::Our,
                Some("test/demo"),
            ),
        ],
    );
    let load = |dsp_base| LoadTable {
        modules: vec![Load {
            id: "test/demo".into(),
            base: 0x2100_0000,
            size: 0x200,
            dsp_base,
            fini: None,
        }],
    };
    let r = Resolver::new(std::slice::from_ref(&map), &load(Some(0xb8_1000))).unwrap();
    let hit = r.lookup(Cpu::Sharc, 0xb8_1014).unwrap();
    assert_eq!((hit.name.as_str(), hit.offset), ("demo_render", 4));
    assert_eq!(hit.module.as_deref(), Some("test/demo"));
    assert!(
        r.lookup(Cpu::Sharc, 0x10).is_none(),
        "not at the link-relative address"
    );
    assert_eq!(name(&r, Cpu::Cf, 0x2100_0030), Some("demo_tick".into()));
    // a module without a DSP base keeps its ColdFire names and leaves its DSP names out; it is not reported as unloaded
    let r = Resolver::new(&[map], &load(None)).unwrap();
    assert!(r.lookup(Cpu::Sharc, 0xb8_1014).is_none());
    assert_eq!(name(&r, Cpu::Cf, 0x2100_0030), Some("demo_tick".into()));
    assert!(r.unloaded.is_empty());
}

#[test]
fn an_old_load_table_without_dsp_base_parses() {
    let l: LoadTable = serde_json::from_str(r#"{"modules":[{"id":"a","base":16}]}"#).unwrap();
    assert_eq!(l.modules[0].dsp_base, None);
    let l: LoadTable =
        serde_json::from_str(r#"{"modules":[{"id":"a","base":16,"dsp_base":12058624}]}"#).unwrap();
    assert_eq!(l.modules[0].dsp_base, Some(0xb8_0000));
}

fn words(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|x| x.to_be_bytes()).collect()
}

/// A State of 3 slots: slot 0 a module with a DSP part, slot 1 free, slot 2 a module without one.
fn synthetic() -> (snapshot::Loader, snapshot::Snapshot) {
    let loader = snapshot::Loader {
        modules: 3,
        l2_byte_base: 0x2000_0000,
        l2_sw_base: 0xb8_0000,
        dsp: true,
        providers: 0,
    };
    let record = |used: u32, name: &str, base: u32, size: u32, fini: u32| {
        let mut n = name.as_bytes().to_vec();
        n.resize(32, 0);
        let mut r = words(&[used]);
        r.extend(n);
        r.extend(words(&[0x0001_0000, base, size, fini]));
        r
    };
    let mut state = words(&[0; 10]);
    state.extend(record(1, "test/dsp-demo", 0x4750_0000, 0x900, 0x4750_0040));
    state.extend(record(0, "", 0, 0, 0));
    state.extend(record(1, "test/dyn-demo", 0x4751_0000, 0x200, 0));
    // DKRT_DSP: 13 header words, mod_off[3], mod_len[3]; slot 0 sits 0x400 bytes into the area and is 0x300 bytes long
    let mut dsp = words(&[0; 13]);
    dsp.extend(words(&[0x400, 0, 0, 0x300, 0, 0]));
    // dyndsp_info: magic, version, device, area
    let info = words(&[0x1234, 1, 2, 0x2001_0000]);
    let snap = [
        ("DKRT_STATE", state),
        ("DKRT_DSP", dsp),
        ("dyndsp_info", info),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    (loader, snap)
}

#[test]
fn a_snapshot_decodes_to_loads_with_and_without_dsp_code() {
    let (loader, snap) = synthetic();
    let lens: Vec<_> = loader.reads().iter().map(|r| (r.symbol, r.len)).collect();
    assert_eq!(
        lens,
        [
            ("DKRT_STATE", 40 + 3 * 52),
            ("DKRT_HOST_DIR", host_dir::BYTES),
            ("DKRT_DSP", 52 + 24),
            ("dyndsp_info", 16)
        ],
        "the reads match what the decoder needs"
    );
    let t = snapshot::decode_loads(&loader, &snap).unwrap();
    assert_eq!(t.modules.len(), 2, "the free slot is skipped");
    let pluck = &t.modules[0];
    assert_eq!(
        (pluck.id.as_str(), pluck.base, pluck.size),
        ("test/dsp-demo", 0x4750_0000, 0x900)
    );
    assert_eq!(pluck.fini, Some(0x4750_0040));
    // area 0x20010000 + 0x400 is 0x10400 bytes into L2: 0x8200 short words
    assert_eq!(pluck.dsp_base, Some(0xb8_0000 + 0x8200));
    let demo = &t.modules[1];
    assert_eq!(
        (demo.id.as_str(), demo.dsp_base, demo.fini),
        ("test/dyn-demo", None, None)
    );
}

#[test]
fn a_snapshot_without_the_dsp_loader_gives_no_dsp_bases() {
    let (mut loader, mut snap) = synthetic();
    snap.remove("DKRT_DSP");
    snap.remove("dyndsp_info");
    loader.dsp = false;
    assert_eq!(loader.reads().len(), 1);
    let t = snapshot::decode_loads(&loader, &snap).unwrap();
    assert!(t.modules.iter().all(|m| m.dsp_base.is_none()));
    snap.insert("DKRT_STATE".into(), vec![0; 41]);
    assert!(snapshot::decode_loads(&loader, &snap).is_err());
    assert!(
        snapshot::parse_hex("0aff").unwrap() == [10, 255] && snapshot::parse_hex("0a f").is_err()
    );
}

#[test]
fn a_provider_record_decodes_to_a_load_and_an_old_loader_header_still_parses() {
    let (mut loader, mut snap) = synthetic();
    loader.providers = 2;
    let mut name = b"core/services/fx".to_vec();
    name.resize(32, 0);
    let mut recs = words(&[1]);
    recs.extend(name);
    recs.extend(words(&[0x0001_0000, 0x4760_0000, 0x1200, 0]));
    recs.extend(words(&[0; 13]));
    snap.insert("DKRT_PROVIDERS".into(), recs);
    let reads: Vec<_> = loader.reads().iter().map(|r| (r.symbol, r.len)).collect();
    assert_eq!(reads.last(), Some(&("DKRT_PROVIDERS", 2 * 52)));
    assert!(
        loader
            .reads()
            .last()
            .is_some_and(|r| r.host_id == Some(gen_host_dir::HOST_PROVIDERS))
    );
    let t = snapshot::decode_loads(&loader, &snap).unwrap();
    assert_eq!(t.modules.len(), 3, "two modules and the provider");
    let fx = &t.modules[2];
    assert_eq!(
        (fx.id.as_str(), fx.base, fx.size, fx.dsp_base),
        ("core/services/fx", 0x4760_0000, 0x1200, None)
    );
    let old: snapshot::Loader =
        serde_json::from_str(r#"{"modules":3,"l2_byte_base":1,"l2_sw_base":2}"#).unwrap();
    assert_eq!((old.providers, old.dsp), (0, false));
    assert_eq!(old.reads().len(), 1);
}

/// A device's memory as a byte image at a base, and the directory a booted core would hold in it.
struct FakeMemory {
    base: u64,
    bytes: Vec<u8>,
}

impl FakeMemory {
    fn peek(&self, at: u64, len: u64) -> Result<Vec<u8>, String> {
        let from = at.checked_sub(self.base).ok_or("below the image")? as usize;
        self.bytes
            .get(from..from + len as usize)
            .map(<[u8]>::to_vec)
            .ok_or_else(|| format!("peek {at:#x}+{len}: outside the image"))
    }
    fn put(&mut self, at: u64, data: &[u8]) {
        let from = (at - self.base) as usize;
        self.bytes[from..from + data.len()].copy_from_slice(data);
    }
}

/// The synthetic snapshot laid out in a fake memory: the resident symbols at fixed addresses, the package statics elsewhere, the directory pointing at them (or empty).
fn memory_with(
    package_loaded: bool,
) -> (
    snapshot::Loader,
    std::collections::BTreeMap<String, u64>,
    FakeMemory,
) {
    memory_with_ready(package_loaded, package_loaded)
}

/// `memory_with`, with the ready word of the directory chosen apart from the entries: the core marks it ready last.
fn memory_with_ready(
    package_loaded: bool,
    ready: bool,
) -> (
    snapshot::Loader,
    std::collections::BTreeMap<String, u64>,
    FakeMemory,
) {
    let (mut loader, snap) = synthetic();
    loader.providers = 1;
    let mut mem = FakeMemory {
        base: 0x4000_0000,
        bytes: vec![0; 0x2000],
    };
    let (state, dir_at, info) = (0x4000_0000u64, 0x4000_0400u64, 0x4000_0500u64);
    let (dsp_at, prov_at) = (0x4000_1000u64, 0x4000_1400u64);
    mem.put(state, &snap["DKRT_STATE"]);
    mem.put(info, &snap["dyndsp_info"]);
    let mut directory = words(&[
        gen_host_dir::HOST_DIR_MAGIC,
        gen_host_dir::HOST_COUNT as u32,
        if ready { gen_host_dir::HOST_DIR_READY } else { 0 },
    ]);
    directory.extend(words(&[0; gen_host_dir::HOST_COUNT]));
    if package_loaded {
        mem.put(dsp_at, &snap["DKRT_DSP"]);
        let mut at = |id: u32, addr: u64| {
            let o = 12 + 4 * id as usize;
            directory[o..o + 4].copy_from_slice(&(addr as u32).to_be_bytes());
        };
        at(gen_host_dir::HOST_DSP, dsp_at);
        at(gen_host_dir::HOST_PROVIDERS, prov_at);
    }
    mem.put(dir_at, &directory);
    let symbols = [
        ("DKRT_STATE", state),
        ("DKRT_HOST_DIR", dir_at),
        ("dyndsp_info", info),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    (loader, symbols, mem)
}

#[test]
fn a_package_static_is_read_where_the_directory_says_it_is() {
    let (loader, symbols, mem) = memory_with(true);
    let snap = loader
        .collect(&symbols, |at, len| mem.peek(at, len))
        .unwrap();
    let (_, want) = synthetic();
    assert_eq!(
        snap["DKRT_DSP"], want["DKRT_DSP"],
        "the DSP table comes from the address of its id, not from a symbol"
    );
    let t = snapshot::decode_loads(&loader, &snap).unwrap();
    assert_eq!(t.modules[0].dsp_base, Some(0xb8_0000 + 0x8200));
}

#[test]
fn an_empty_entry_says_no_package_is_loaded() {
    let (loader, symbols, mem) = memory_with(false);
    let err = loader
        .collect(&symbols, |at, len| mem.peek(at, len))
        .unwrap_err();
    assert!(
        err.contains("no package loaded") && err.contains("DKRT_DSP"),
        "{err}"
    );
}

/// The core clears the directory, fills its own entries and the package fills its own; only then does it mark the directory ready. A tool that reads in between (the entries
/// are there, the ready word is not) finds no package.
#[test]
fn a_directory_that_is_not_ready_says_no_package_even_with_entries_filled() {
    let (loader, symbols, mem) = memory_with_ready(true, false);
    let err = loader
        .collect(&symbols, |at, len| mem.peek(at, len))
        .unwrap_err();
    assert!(
        err.contains("no package loaded") && err.contains("still starting"),
        "{err}"
    );
    let (loader, symbols, mem) = memory_with_ready(true, true);
    assert!(loader
        .collect(&symbols, |at, len| mem.peek(at, len))
        .is_ok());
}

#[test]
fn the_directory_reader_tells_empty_old_and_foreign_bytes_apart() {
    let mut d = words(&[
        gen_host_dir::HOST_DIR_MAGIC,
        1,
        gen_host_dir::HOST_DIR_READY,
    ]);
    d.extend(words(
        &[0x4000_2000, 0, 0, 0][..gen_host_dir::HOST_COUNT.min(4)],
    ));
    let dir = host_dir::HostDir::parse(&d).unwrap();
    assert_eq!(dir.address(gen_host_dir::HOST_DSP), Ok(0x4000_2000));
    // a core whose count is 1 has no later id
    assert!(matches!(
        dir.address(gen_host_dir::HOST_PROVIDERS),
        Err(host_dir::Error::UnknownId(_))
    ));
    assert_eq!(
        dir.address(gen_host_dir::HOST_COUNT as u32),
        Err(host_dir::Error::BadId(gen_host_dir::HOST_COUNT as u32))
    );
    assert!(matches!(
        host_dir::HostDir::parse(&words(&[1, 2])),
        Err(host_dir::Error::NotADirectory)
    ));
    assert!(matches!(
        host_dir::HostDir::parse(&[0; 4]),
        Err(host_dir::Error::NotADirectory)
    ));
}
