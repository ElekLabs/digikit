//! Run-time modules: `dynamic = true` modules, the `.dkm` register records, the reserved rows of `core/dynreg`
//! (docs/design/digikit-registry.md). The image tests are skipped when the stock images are absent.

use patchkit::apply;
use patchkit::dkm;
use patchkit::module::{AllocTable, Library};
use patchkit::port::Registry;
use patchkit::register::{KNOB_PARAM, Register, machine_hash, machine_key};
use patchkit::typed::{Models, resolve};
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn library() -> Library {
    let mut lib = Library::scan_repo(&root()).unwrap();
    lib.lower(&Models::from_dir(&root().join("devices")).unwrap())
        .unwrap();
    lib
}

#[test]
fn a_dynamic_module_is_not_lowered_and_not_patched() {
    let lib = library();
    let m = lib.get("test/dyn-xslice").unwrap();
    assert!(m.manifest.module.dynamic);
    assert!(
        m.manifest.entry.is_empty(),
        "no registry rows are made for a run-time module"
    );
    let reg = Registry::from_dir(&root().join("devices/ports")).unwrap();
    for file in ["Digitakt_II_OS1.17.syx"] {
        let Ok(raw) = std::fs::read(root().join("firmware").join(file)) else {
            continue;
        };
        let e = apply::apply(&raw, &reg, &lib, &["test/dyn-xslice".to_string()])
            .err()
            .expect("refused");
        assert!(e.0.contains("run-time module"), "{e}");
    }
}

#[test]
fn the_register_records_are_the_lowered_typed_sections() {
    let lib = library();
    let m = lib.get("test/dyn-xslice").unwrap();
    let models = Models::from_dir(&root().join("devices")).unwrap();
    let res = resolve(&m.manifest, &models).unwrap();
    // only the Digitakt: the module says `devices = ["dt2"]`
    assert_eq!(
        res.iter().map(|r| r.dev.as_str()).collect::<Vec<_>>(),
        ["dt2"]
    );
    let reg = Register::lower(&res[0], "test/dyn-xslice").unwrap();
    assert_eq!(reg.device, 1);
    assert_eq!(reg.machines.len(), 1);
    let mach = &reg.machines[0];
    assert_eq!(
        (mach.type_no, mach.clone_of, mach.position, mach.level),
        (machine_key("test/dyn-xslice", "xslice_d", 16), 6, 255, 10)
    );
    // knobs: XSLICE's stock codes, knob 6 the module's own parameter (index 0)
    assert_eq!(
        mach.knobs,
        [0xf8, 0xf9, 0xfa, 0xfb, 0xfc, 0xfd, KNOB_PARAM, 0xfe]
    );
    let p = &reg.params[0];
    assert_eq!(
        (p.template, p.slot, p.min, p.max, p.default),
        (0xd1, 6, 0, 0x7f00, 0x2000)
    );
    // the file: a stub image, the records, and the same bytes every time
    let opts = dkm::Options {
        name: "test/dyn-xslice".into(),
        version: 0x000100,
        runtime_min: 1,
    };
    let a = dkm::pack_with(None, &reg.encode(), &opts).unwrap();
    assert_eq!(a, dkm::pack_with(None, &reg.encode(), &opts).unwrap());
    let d = dkm::Dkm::parse(&a).unwrap();
    assert_eq!(d.init, Some(0));
    assert_eq!(Register::parse(&d.register).unwrap(), reg);
    // the key is the 24-bit FNV-1a hash of "<module id>:<machine id>" above a zero type byte (D7)
    assert_eq!(mach.type_no & 0xff, 16); // its type_hint
    assert_eq!(machine_hash("a", "b"), 0xbd8540);
}

#[test]
fn the_run_time_type_range_is_checked() {
    let ok =
        "[dt2]\nlast = 23\ndynamic_first = 16\n[dt2.alloc]\n\"a:x\" = 7\n\"core/dynreg:d\" = 16\n";
    assert!(AllocTable::parse(ok).is_ok());
    let static_in_range = ok.replace("\"a:x\" = 7", "\"a:x\" = 18");
    assert!(
        AllocTable::parse(&static_in_range)
            .unwrap_err()
            .0
            .contains("run-time range")
    );
    // run-time machines have no number in the table any more: `[dt2.dynamic]` is refused
    let old = format!("{ok}[dt2.dynamic]\n\"m:y\" = 17\n");
    assert!(AllocTable::parse(&old).is_err());
}

/// The framework reserves 8 machine types (16 .. 23) and 16 parameter codes, with the runtime's table of addresses; the
/// runtime alone still patches (its link to the table is optional).
#[test]
#[ignore = "this fork retires the run-time machine range (core/machines types.toml)"]
fn dynreg_reserves_rows_and_the_runtime_links_optionally() {
    let lib = library();
    let reg = Registry::from_dir(&root().join("devices/ports")).unwrap();
    let mut ran = 0;
    for file in ["Digitakt_II_OS1.17.syx"] {
        let Ok(raw) = std::fs::read(root().join("firmware").join(file)) else {
            continue;
        };
        let want = |ids: &[&str]| -> Vec<String> {
            lib.with_requires(&ids.iter().map(|s| s.to_string()).collect::<Vec<_>>())
                .unwrap()
                .0
        };
        let out = apply::apply(&raw, &reg, &lib, &want(&["core/runtime", "core/dynreg"])).unwrap();
        let plan = serde_json::to_value(&out.plan).unwrap();
        let sym = |n: &str| -> u64 {
            plan["symbols"]
                .as_object()
                .unwrap()
                .iter()
                .find(|(k, _)| k.split(':').nth(1) == Some(n))
                .unwrap_or_else(|| panic!("no symbol {n}"))
                .1
                .as_u64()
                .unwrap()
        };
        assert_eq!(sym("machines.dyn16.index"), 16);
        assert_eq!(sym("machines.dyn23.index"), 23);
        assert_eq!(sym("machines.count"), 17, "types 7 .. 23");
        assert_eq!(sym("params.dp15.index"), sym("params.dp0.index") + 15);
        assert!(sym("dynreg_info") > 0x4000_0000);
        // the runtime alone: the optional link resolves to 0, nothing else changes
        let alone = apply::apply(&raw, &reg, &lib, &want(&["core/runtime"])).unwrap();
        let alone_plan = serde_json::to_value(&alone.plan).unwrap();
        assert!(
            alone_plan["symbols"]
                .as_object()
                .unwrap()
                .keys()
                .all(|k| !k.contains("dynreg"))
        );
        ran += 1;
    }
    eprintln!("dynreg: {ran} images");
}

/// `type_hint` (docs/findings/52-sticky-machine-ids.md): the machine key carries it in the low byte, per device, and the pack refuses a hint
/// outside the device's run-time range.
#[test]
fn a_type_hint_is_the_low_byte_of_the_machine_key_and_is_range_checked() {
    use patchkit::register::{check_type_hints, fmt_type};
    let lib = library();
    let models = Models::from_dir(&root().join("devices")).unwrap();
    let m = lib.get("digikit/pluck").unwrap();
    let res = resolve(&m.manifest, &models).unwrap();
    let key = |dev: &str| {
        let r = res.iter().find(|r| r.dev == dev).unwrap();
        Register::lower(r, "digikit/pluck").unwrap().machines[0].type_no
    };
    let h = machine_hash("digikit/pluck", "pluck") << 8;
    assert_eq!(key("dt2"), h | 18);
    assert_eq!(key("dn2"), h | 17);
    assert_eq!(fmt_type(key("dt2")), format!("h{:06x}+18", h >> 8));
    // a machine without a hint keeps the low byte 0
    let text =
        std::fs::read_to_string(root().join("tests/fixtures/modules/dyn-xslice/module.toml"))
            .unwrap();
    let none = text.replace("type_hint = 16", "");
    let mx: patchkit::module::Manifest = toml::from_str(&none).unwrap();
    let rx = resolve(&mx, &models).unwrap();
    assert_eq!(
        Register::lower(&rx[0], "test/dyn-xslice").unwrap().machines[0].type_no & 0xff,
        0
    );
    // the range: 16 ..= 23 is fine, 15 and 24 are pack errors that name the machine
    let dt2 = res.iter().find(|r| r.dev == "dt2").unwrap();
    assert!(check_type_hints(dt2, "digikit/pluck", 16, 23).is_ok());
    for (first, last) in [(19, 23), (16, 17)] {
        let e = check_type_hints(dt2, "digikit/pluck", first, last)
            .unwrap_err()
            .0;
        assert!(e.contains("type_hint 18") && e.contains("pluck"), "{e}");
    }
    // a hint of 300 is not a type number
    let bad = std::fs::read_to_string(root().join("modules/machines/pluck/module.toml"))
        .unwrap()
        .replace("type_hint = 18", "type_hint = 300");
    let mf: patchkit::module::Manifest = toml::from_str(&bad).unwrap();
    assert!(
        resolve(&mf, &models)
            .unwrap_err()
            .0
            .contains("type_hint 300")
    );
}
